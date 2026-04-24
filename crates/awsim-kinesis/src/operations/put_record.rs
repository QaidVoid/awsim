use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::debug;

use crate::state::{KinesisRecord, KinesisState, now_millis};
use crate::util::{hash_to_shard_index, partition_key_to_hash};

pub fn handle(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let data = input["Data"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Data is required"))?;

    let partition_key = input["PartitionKey"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "PartitionKey is required"))?;

    let explicit_hash_key = input["ExplicitHashKey"].as_str().map(|s| s.to_string());

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    // Determine hash key: explicit or derived from partition key
    let hash = if let Some(ref ehk) = explicit_hash_key {
        ehk.parse::<u128>()
            .unwrap_or_else(|_| partition_key_to_hash(partition_key))
    } else {
        partition_key_to_hash(partition_key)
    };

    let shard_index = hash_to_shard_index(hash, &stream.shards);
    let shard = &mut stream.shards[shard_index];
    let sequence_number = shard.alloc_sequence();
    let shard_id = shard.shard_id.clone();

    // explicit_hash_key is used for routing but not stored per record
    let _ = explicit_hash_key;

    let record = KinesisRecord {
        sequence_number: sequence_number.clone(),
        data: data.to_string(),
        partition_key: partition_key.to_string(),
        timestamp_millis: now_millis(),
    };

    debug!(
        stream = %stream_name,
        shard = %shard_id,
        seq = %sequence_number,
        "PutRecord"
    );

    shard.records.push(record);

    Ok(json!({
        "ShardId": shard_id,
        "SequenceNumber": sequence_number,
        "EncryptionType": "NONE",
    }))
}
