use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, ShardIteratorInfo, now_millis};
use crate::util::{decode_iterator, encode_iterator};

pub fn handle(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
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
        AwsError::bad_request(
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

    let shard_id = stream.shards[info.shard_index].shard_id.clone();
    let sqlite = state
        .sqlite()
        .ok_or_else(|| AwsError::internal("Kinesis sqlite store not initialised"))?;

    let rows = sqlite.read_after(
        &ctx.account_id,
        &ctx.region,
        &info.stream_name,
        &shard_id,
        info.position as i64,
        limit,
    )?;

    // AWS caps a single GetRecords response at 10 MB of record data;
    // anything past that boundary spills into the next call. Stop
    // including records once the running total would exceed the cap,
    // and advance the cursor to the last record we actually included.
    const KINESIS_GET_RECORDS_MAX_BYTES: usize = 10 * 1024 * 1024;
    let mut bytes_so_far: usize = 0;
    let mut last_included_seq: Option<u64> = None;
    let mut records: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let record_bytes = r.data.len() + r.partition_key.len();
        if !records.is_empty() && bytes_so_far + record_bytes > KINESIS_GET_RECORDS_MAX_BYTES {
            break;
        }
        bytes_so_far += record_bytes;
        last_included_seq = Some(r.seq as u64);
        records.push(json!({
            "SequenceNumber": format!("{:020}", r.seq),
            "ApproximateArrivalTimestamp": r.timestamp_millis / 1000,
            "Data": r.data,
            "PartitionKey": r.partition_key,
            "EncryptionType": "NONE",
        }));
    }

    let new_position = last_included_seq.unwrap_or(info.position);

    let next_iterator_info = ShardIteratorInfo {
        stream_name: info.stream_name.clone(),
        shard_index: info.shard_index,
        position: new_position,
    };
    let next_token = encode_iterator(&next_iterator_info);
    state
        .iterators
        .insert(next_token.clone(), next_iterator_info);

    // MillisBehindLatest: rough lag between our cursor and the most
    // recent record's wall-clock timestamp.
    let max_seq = sqlite
        .max_seq(&ctx.account_id, &ctx.region, &info.stream_name, &shard_id)
        .unwrap_or(0) as u64;
    let millis_behind = if new_position >= max_seq {
        0u64
    } else {
        // Fetch the next unread record's timestamp to estimate the lag.
        let next_unread = sqlite
            .read_after(
                &ctx.account_id,
                &ctx.region,
                &info.stream_name,
                &shard_id,
                new_position as i64,
                1,
            )
            .ok()
            .and_then(|v| v.into_iter().next());
        match next_unread {
            Some(r) => now_millis().saturating_sub(r.timestamp_millis as u64),
            None => 0,
        }
    };

    Ok(json!({
        "Records": records,
        "NextShardIterator": next_token,
        "MillisBehindLatest": millis_behind,
    }))
}
