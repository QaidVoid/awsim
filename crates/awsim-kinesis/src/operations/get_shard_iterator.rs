use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, ShardIteratorInfo};
use crate::util::encode_iterator;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let shard_id = input["ShardId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ShardId is required"))?;

    let iterator_type = input["ShardIteratorType"].as_str().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "ShardIteratorType is required")
    })?;

    let stream = state.streams.get(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    // Find shard index by shard_id
    let shard_index = stream
        .shards
        .iter()
        .position(|s| s.shard_id == shard_id)
        .ok_or_else(|| {
            AwsError::bad_request(
                "ResourceNotFoundException",
                format!(
                    "Shard {} does not exist in stream {}",
                    shard_id, stream_name
                ),
            )
        })?;

    let shard = &stream.shards[shard_index];

    let position = match iterator_type {
        "TRIM_HORIZON" => 0,
        "LATEST" => shard.records.len(),
        "AT_SEQUENCE_NUMBER" => {
            let seq = input["StartingSequenceNumber"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "MissingParameter",
                    "StartingSequenceNumber is required for AT_SEQUENCE_NUMBER",
                )
            })?;
            shard
                .records
                .iter()
                .position(|r| r.sequence_number == seq)
                .unwrap_or(shard.records.len())
        }
        "AFTER_SEQUENCE_NUMBER" => {
            let seq = input["StartingSequenceNumber"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "MissingParameter",
                    "StartingSequenceNumber is required for AFTER_SEQUENCE_NUMBER",
                )
            })?;
            shard
                .records
                .iter()
                .position(|r| r.sequence_number == seq)
                .map(|i| i + 1)
                .unwrap_or(shard.records.len())
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("Unknown ShardIteratorType: {}", other),
            ));
        }
    };

    let info = ShardIteratorInfo {
        stream_name: stream_name.to_string(),
        shard_index,
        position,
    };

    let token = encode_iterator(&info);
    // Store it for GetRecords to look up (not strictly needed since we decode from the token,
    // but useful for debugging / future TTL expiry)
    state.iterators.insert(token.clone(), info);

    Ok(json!({
        "ShardIterator": token,
    }))
}
