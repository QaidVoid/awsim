use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let stream = state.streams.get(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    // AWS caps Limit at 10_000 (default 100) and paginates by
    // ExclusiveStartShardId. Closed shards (those with an
    // EndingSequenceNumber set) are surfaced just like open shards,
    // since DescribeStream is the legacy "give me everything" API —
    // callers filter with ListShards.ShardFilter when they only want
    // open shards.
    let limit = match input["Limit"].as_i64() {
        Some(n) if !(1..=10_000).contains(&n) => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                "Limit must be between 1 and 10000.",
            ));
        }
        Some(n) => n as usize,
        None => 100,
    };
    let start_after = input["ExclusiveStartShardId"].as_str();

    let mut shards = stream.shards.clone();
    let retention_hours = stream.retention_hours;
    let stream_name_out = stream.name.clone();
    let stream_arn = stream.arn.clone();
    let stream_status = stream.status.clone();
    let created_at = stream.created_at;
    drop(stream);

    shards.sort_by(|a, b| a.shard_id.cmp(&b.shard_id));
    if let Some(start) = start_after {
        let idx = shards
            .iter()
            .position(|s| s.shard_id == start)
            .map(|i| i + 1)
            .unwrap_or(0);
        shards.drain(..idx);
    }
    let has_more_shards = shards.len() > limit;
    shards.truncate(limit);

    let shards_json: Vec<Value> = shards
        .iter()
        .map(|s| {
            let mut seq_range = json!({
                "StartingSequenceNumber": s.sequence_number_range.0,
            });
            if let Some(ref end) = s.sequence_number_range.1 {
                seq_range["EndingSequenceNumber"] = Value::String(end.clone());
            }
            json!({
                "ShardId": s.shard_id,
                "HashKeyRange": {
                    "StartingHashKey": s.hash_key_range.0,
                    "EndingHashKey": s.hash_key_range.1,
                },
                "SequenceNumberRange": seq_range,
            })
        })
        .collect();

    Ok(json!({
        "StreamDescription": {
            "StreamName": stream_name_out,
            "StreamARN": stream_arn,
            "StreamStatus": stream_status,
            "Shards": shards_json,
            "HasMoreShards": has_more_shards,
            "RetentionPeriodHours": retention_hours,
            "StreamCreationTimestamp": created_at,
            "EnhancedMonitoring": [],
            "EncryptionType": "NONE",
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KinesisState, KinesisStream, Shard};

    fn ctx() -> RequestContext {
        RequestContext::new("kinesis", "us-east-1")
    }

    fn stream_with_shards(state: &KinesisState, name: &str, n: usize) {
        let mut shards = Vec::new();
        for i in 0..n {
            shards.push(Shard::new_range(i, i as u128, (i as u128) + 1));
        }
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
        };
        state.streams.insert(name.to_string(), stream);
    }

    #[test]
    fn describe_stream_paginates_with_exclusive_start_shard_id() {
        let state = KinesisState::default();
        stream_with_shards(&state, "s", 5);

        // First page: limit 2 → has_more=true, returns first 2 shards.
        let resp = handle(&state, &json!({ "StreamName": "s", "Limit": 2 }), &ctx()).unwrap();
        assert_eq!(resp["StreamDescription"]["HasMoreShards"], json!(true));
        let shards = resp["StreamDescription"]["Shards"].as_array().unwrap();
        assert_eq!(shards.len(), 2);
        let last = shards.last().unwrap()["ShardId"].as_str().unwrap();

        // Second page starts after the last id.
        let resp = handle(
            &state,
            &json!({
                "StreamName": "s",
                "Limit": 2,
                "ExclusiveStartShardId": last,
            }),
            &ctx(),
        )
        .unwrap();
        let shards2 = resp["StreamDescription"]["Shards"].as_array().unwrap();
        assert_eq!(shards2.len(), 2);
        assert_ne!(shards2[0]["ShardId"], shards[0]["ShardId"]);
    }

    #[test]
    fn describe_stream_rejects_limit_out_of_range() {
        let state = KinesisState::default();
        stream_with_shards(&state, "s", 1);
        let err = handle(
            &state,
            &json!({ "StreamName": "s", "Limit": 20_000 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }
}
