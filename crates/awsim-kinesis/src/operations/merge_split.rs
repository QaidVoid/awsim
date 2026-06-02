use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, Shard, shard_id_for};

pub fn handle_merge(
    _state: &KinesisState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // MergeShards is a stub — resharding is complex and not required for local dev
    Ok(json!({}))
}

/// SplitShard closes the source shard and inserts two child shards
/// that cover the lower and upper halves of the parent's hash-key
/// range. Child shard IDs follow the documented pattern
/// `shardId-NNNNNNNNNNNN` with the suffix monotonically increasing
/// within the stream so a long-running stream's history stays
/// reconstructable from id order.
pub fn handle_split(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;
    let shard_to_split = input["ShardToSplit"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ShardToSplit is required"))?;
    let new_starting_hash_key = input["NewStartingHashKey"].as_str().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "NewStartingHashKey is required")
    })?;

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    let new_key: u128 = new_starting_hash_key.parse().map_err(|_| {
        AwsError::bad_request(
            "InvalidArgumentException",
            "NewStartingHashKey must be a decimal integer in 0..=2^128-1.",
        )
    })?;

    let src_idx = stream
        .shards
        .iter()
        .position(|s| s.shard_id == shard_to_split)
        .ok_or_else(|| {
            AwsError::bad_request(
                "ResourceNotFoundException",
                format!("ShardToSplit `{shard_to_split}` not found on stream `{stream_name}`."),
            )
        })?;

    {
        let src = &stream.shards[src_idx];
        if src.sequence_number_range.1.is_some() {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("ShardToSplit `{shard_to_split}` is already closed."),
            ));
        }
        let start: u128 = src.hash_key_range.0.parse().unwrap_or(0);
        let end: u128 = src.hash_key_range.1.parse().unwrap_or(0);
        // NewStartingHashKey must fall strictly inside the parent
        // range; equality at the edges would produce a zero-width
        // child, which AWS rejects.
        if new_key <= start || new_key > end {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!(
                    "NewStartingHashKey {new_key} must satisfy {start} < key <= {end} \
                     for shard `{shard_to_split}`."
                ),
            ));
        }
    }

    let (parent_start, parent_end) = {
        let src = &stream.shards[src_idx];
        (src.hash_key_range.0.clone(), src.hash_key_range.1.clone())
    };

    // Close the parent: stamp its EndingSequenceNumber with the next
    // sequence allocation so GetRecords on the closed shard sees the
    // cutoff.
    let close_seq = stream.shards[src_idx].next_sequence;
    stream.shards[src_idx].sequence_number_range.1 = Some(format!("{close_seq:020}"));

    // Choose monotonic ids for the two children. The next available
    // numeric suffix is one past the max suffix currently in use, so
    // resharding never reuses an id even after a parent is dropped.
    let next_index = stream
        .shards
        .iter()
        .filter_map(|s| s.shard_id.strip_prefix("shardId-"))
        .filter_map(|n| n.parse::<usize>().ok())
        .max()
        .map(|n| n + 1)
        .unwrap_or(stream.shards.len());

    let child_lo = Shard {
        shard_id: shard_id_for(next_index),
        hash_key_range: (parent_start, (new_key - 1).to_string()),
        sequence_number_range: (format!("{close_seq:020}"), None),
        next_sequence: close_seq + 1,
    };
    let child_hi = Shard {
        shard_id: shard_id_for(next_index + 1),
        hash_key_range: (new_key.to_string(), parent_end),
        sequence_number_range: (format!("{close_seq:020}"), None),
        next_sequence: close_seq + 1,
    };
    stream.shards.push(child_lo);
    stream.shards.push(child_hi);
    Ok(json!({}))
}

#[cfg(test)]
mod split_shard_tests {
    use super::*;
    use crate::state::{KinesisState, KinesisStream, Shard};

    fn ctx() -> RequestContext {
        RequestContext::new("kinesis", "us-east-1")
    }

    fn seed(state: &KinesisState, name: &str) {
        let shards = vec![Shard {
            shard_id: "shardId-000000000000".to_string(),
            hash_key_range: (
                "0".to_string(),
                "340282366920938463463374607431768211455".to_string(),
            ),
            sequence_number_range: ("00000000000000000000".to_string(), None),
            next_sequence: 7,
        }];
        let stream = KinesisStream {
            name: name.to_string(),
            arn: format!("arn:aws:kinesis:us-east-1:000000000000:stream/{name}"),
            status: "ACTIVE".to_string(),
            shards,
            retention_hours: 24,
            tags: Default::default(),
            created_at: 0,
            enhanced_monitoring: vec![],
            encryption_type: "NONE".to_string(),
            key_id: None,
            stream_mode: "PROVISIONED".to_string(),
            warm_throughput_mibps: 0,
            warm_throughput_records: 0,
            pending_update: None,
        };
        state.streams.insert(name.to_string(), stream);
    }

    #[test]
    fn split_closes_parent_and_assigns_monotonic_child_ids() {
        let state = KinesisState::default();
        seed(&state, "s");
        handle_split(
            &state,
            &json!({
                "StreamName": "s",
                "ShardToSplit": "shardId-000000000000",
                "NewStartingHashKey": "170141183460469231731687303715884105728",
            }),
            &ctx(),
        )
        .unwrap();
        let stream = state.streams.get("s").unwrap();
        assert_eq!(stream.shards.len(), 3);
        let parent = &stream.shards[0];
        assert!(
            parent.sequence_number_range.1.is_some(),
            "parent must be closed"
        );
        let lo = &stream.shards[1];
        let hi = &stream.shards[2];
        assert_eq!(lo.shard_id, "shardId-000000000001");
        assert_eq!(hi.shard_id, "shardId-000000000002");
        // Children cover the full range without overlap.
        assert_eq!(lo.hash_key_range.0, "0");
        assert_eq!(
            lo.hash_key_range.1,
            "170141183460469231731687303715884105727"
        );
        assert_eq!(
            hi.hash_key_range.0,
            "170141183460469231731687303715884105728"
        );
        assert_eq!(
            hi.hash_key_range.1,
            "340282366920938463463374607431768211455"
        );
    }

    #[test]
    fn rejects_split_outside_parent_range() {
        let state = KinesisState::default();
        seed(&state, "s");
        let err = handle_split(
            &state,
            &json!({
                "StreamName": "s",
                "ShardToSplit": "shardId-000000000000",
                "NewStartingHashKey": "0",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn rejects_split_on_unknown_shard() {
        let state = KinesisState::default();
        seed(&state, "s");
        let err = handle_split(
            &state,
            &json!({
                "StreamName": "s",
                "ShardToSplit": "shardId-999999999999",
                "NewStartingHashKey": "1",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn rejects_split_on_already_closed_shard() {
        let state = KinesisState::default();
        seed(&state, "s");
        handle_split(
            &state,
            &json!({
                "StreamName": "s",
                "ShardToSplit": "shardId-000000000000",
                "NewStartingHashKey": "170141183460469231731687303715884105728",
            }),
            &ctx(),
        )
        .unwrap();
        let err = handle_split(
            &state,
            &json!({
                "StreamName": "s",
                "ShardToSplit": "shardId-000000000000",
                "NewStartingHashKey": "100",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }
}
