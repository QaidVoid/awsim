use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, ShardIteratorInfo, now_millis};
use crate::util::{decode_iterator, encode_iterator};

pub fn handle(state: &KinesisState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let iterator_token = input["ShardIterator"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ShardIterator is required"))?;

    let limit = input["Limit"].as_u64().unwrap_or(10000).min(10000) as usize;

    let info = decode_iterator(iterator_token).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidArgumentException",
            "Invalid or expired ShardIterator",
        )
    })?;

    let stream = state.streams.get(&info.stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", info.stream_name),
        )
    })?;

    if info.shard_index >= stream.shards.len() {
        return Err(AwsError::bad_request(
            "ResourceNotFoundException",
            "Shard does not exist",
        ));
    }

    let shard = &stream.shards[info.shard_index];
    let total_records = shard.records.len();
    let start = info.position.min(total_records);
    let end = (start + limit).min(total_records);

    let records: Vec<Value> = shard.records[start..end]
        .iter()
        .map(|r| {
            json!({
                "SequenceNumber": r.sequence_number,
                "ApproximateArrivalTimestamp": r.timestamp_millis / 1000,
                "Data": r.data,
                "PartitionKey": r.partition_key,
                "EncryptionType": "NONE",
            })
        })
        .collect();

    let new_position = end;

    // Build next iterator at the new position
    let next_iterator_info = ShardIteratorInfo {
        stream_name: info.stream_name.clone(),
        shard_index: info.shard_index,
        position: new_position,
    };
    let next_token = encode_iterator(&next_iterator_info);
    state.iterators.insert(next_token.clone(), next_iterator_info);

    // MillisBehindLatest: 0 if we've caught up to the latest record, else a positive value
    let millis_behind = if new_position >= total_records {
        0u64
    } else {
        // Use approximate lag based on the oldest unread record's timestamp
        let oldest_unread = &shard.records[new_position];
        now_millis().saturating_sub(oldest_unread.timestamp_millis)
    };

    Ok(json!({
        "Records": records,
        "NextShardIterator": next_token,
        "MillisBehindLatest": millis_behind,
    }))
}
