use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::debug;

use crate::state::{KinesisState, now_millis};
use crate::util::{hash_to_shard_index, partition_key_to_hash};

pub fn handle(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
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
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    let hash = if let Some(ref ehk) = explicit_hash_key {
        ehk.parse::<u128>()
            .unwrap_or_else(|_| partition_key_to_hash(partition_key))
    } else {
        partition_key_to_hash(partition_key)
    };

    let shard_index = hash_to_shard_index(hash, &stream.shards);
    let shard = &mut stream.shards[shard_index];
    let (seq_i64, sequence_number) = shard.alloc_sequence();
    let shard_id = shard.shard_id.clone();
    let _ = explicit_hash_key;

    let sqlite = state
        .sqlite()
        .ok_or_else(|| AwsError::internal("Kinesis sqlite store not initialised"))?;
    sqlite.put_record(
        &ctx.account_id,
        &ctx.region,
        stream_name,
        &shard_id,
        seq_i64,
        partition_key,
        data,
        now_millis() as i64,
    )?;

    debug!(
        stream = %stream_name,
        shard = %shard_id,
        seq = %sequence_number,
        "PutRecord"
    );

    Ok(json!({
        "ShardId": shard_id,
        "SequenceNumber": sequence_number,
        "EncryptionType": "NONE",
    }))
}
