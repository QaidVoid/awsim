use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::debug;

use crate::sqlite_store::KinesisRecordRow;
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

    let records = input["Records"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Records is required"))?;

    if records.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "Records must not be empty",
        ));
    }

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    let now_ms = now_millis() as i64;
    let mut rows: Vec<(String, KinesisRecordRow)> = Vec::with_capacity(records.len());
    let mut result_records = Vec::with_capacity(records.len());
    let mut failed_count = 0u64;

    for entry in records {
        let data = match entry["Data"].as_str() {
            Some(d) => d,
            None => {
                failed_count += 1;
                result_records.push(json!({
                    "ErrorCode": "ValidationException",
                    "ErrorMessage": "Data is required",
                }));
                continue;
            }
        };

        let partition_key = match entry["PartitionKey"].as_str() {
            Some(pk) => pk,
            None => {
                failed_count += 1;
                result_records.push(json!({
                    "ErrorCode": "ValidationException",
                    "ErrorMessage": "PartitionKey is required",
                }));
                continue;
            }
        };

        let explicit_hash_key = entry["ExplicitHashKey"].as_str().map(|s| s.to_string());

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

        debug!(
            stream = %stream_name,
            shard = %shard_id,
            seq = %sequence_number,
            "PutRecords entry"
        );

        rows.push((
            shard_id.clone(),
            KinesisRecordRow {
                seq: seq_i64,
                partition_key: partition_key.to_string(),
                data: data.to_string(),
                timestamp_millis: now_ms,
            },
        ));

        result_records.push(json!({
            "ShardId": shard_id,
            "SequenceNumber": sequence_number,
        }));
    }

    let sqlite = state
        .sqlite()
        .ok_or_else(|| AwsError::internal("Kinesis sqlite store not initialised"))?;
    sqlite.put_records(&ctx.account_id, &ctx.region, stream_name, &rows)?;

    Ok(json!({
        "FailedRecordCount": failed_count,
        "Records": result_records,
        "EncryptionType": "NONE",
    }))
}
