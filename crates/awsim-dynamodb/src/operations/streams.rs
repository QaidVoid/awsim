use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::DynamoState;

use super::opt_str;

/// `DescribeStream` — return stream metadata including shards.
///
/// In AWSim we model a single shard per table stream.
pub fn describe_stream(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_arn = require_stream_arn(input)?;

    let table = find_table_by_stream_arn(state, stream_arn)?;

    let shard_id = format!("shardId-00000000000000000000-{}", &table.name);
    let description = json!({
        "StreamArn": stream_arn,
        "StreamLabel": stream_label_from_arn(stream_arn),
        "StreamStatus": "ENABLED",
        "StreamViewType": table.stream_view_type.as_deref().unwrap_or("NEW_AND_OLD_IMAGES"),
        "TableName": table.name,
        "KeySchema": table.key_schema.iter().map(|k| json!({
            "AttributeName": k.attribute_name,
            "KeyType": k.key_type,
        })).collect::<Vec<_>>(),
        "Shards": [{
            "ShardId": shard_id,
            "SequenceNumberRange": {
                "StartingSequenceNumber": "0000000000000000000001",
            },
        }],
        "LastEvaluatedShardId": null,
        "CreationRequestDateTime": table.created_at,
    });

    Ok(json!({ "StreamDescription": description }))
}

/// `GetShardIterator` — return an opaque iterator token for reading records.
///
/// Iterator format: `{stream_arn}|{shard_id}|{seq_start}`
pub fn get_shard_iterator(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_arn = require_stream_arn(input)?;
    let shard_id = input
        .get("ShardId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::validation("ShardId is required"))?;
    let iterator_type = opt_str(input, "ShardIteratorType").unwrap_or("LATEST");

    // Validate the stream exists.
    let _table = find_table_by_stream_arn(state, stream_arn)?;

    // Determine starting sequence number from iterator type.
    let seq_start: u64 = match iterator_type {
        "TRIM_HORIZON" => 0,
        "AT_SEQUENCE_NUMBER" | "AFTER_SEQUENCE_NUMBER" => {
            let raw = input
                .get("SequenceNumber")
                .and_then(|v| v.as_str())
                .unwrap_or("0");
            raw.parse::<u64>().unwrap_or(0)
        }
        // LATEST — point to the next record to be written.
        _ => {
            let table = find_table_by_stream_arn(state, stream_arn)?;
            table.stream_sequence
        }
    };

    // Encode iterator as a pipe-separated token.
    let iterator = format!("{stream_arn}|{shard_id}|{seq_start}");
    Ok(json!({ "ShardIterator": iterator }))
}

/// `GetRecords` — read records from the stream starting at the given iterator.
pub fn get_records(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let iterator = input
        .get("ShardIterator")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::validation("ShardIterator is required"))?;

    let limit = input.get("Limit").and_then(|v| v.as_u64()).unwrap_or(1000) as usize;

    // Parse the iterator token produced by get_shard_iterator.
    let parts: Vec<&str> = iterator.splitn(3, '|').collect();
    if parts.len() != 3 {
        return Err(AwsError::validation("ShardIterator is malformed"));
    }
    let stream_arn = parts[0];
    let shard_id = parts[1];
    let from_seq: u64 = parts[2].parse().unwrap_or(0);

    let table = find_table_by_stream_arn(state, stream_arn)?;

    // Collect records whose sequence number >= from_seq.
    let records: Vec<Value> = table
        .stream_records
        .iter()
        .filter(|r| r.dynamodb.sequence_number.parse::<u64>().unwrap_or(0) >= from_seq)
        .take(limit)
        .map(|r| {
            let mut rec = json!({
                "eventID": r.event_id,
                "eventName": r.event_name,
                "eventSource": "aws:dynamodb",
                "eventSourceARN": r.event_source_arn,
                "eventVersion": "1.1",
                "dynamodb": {
                    "Keys": r.dynamodb.keys,
                    "SequenceNumber": r.dynamodb.sequence_number,
                    "SizeBytes": r.dynamodb.size_bytes,
                    "StreamViewType": r.dynamodb.stream_view_type,
                },
            });
            if let Some(ref new_img) = r.dynamodb.new_image {
                rec["dynamodb"]["NewImage"] = json!(new_img);
            }
            if let Some(ref old_img) = r.dynamodb.old_image {
                rec["dynamodb"]["OldImage"] = json!(old_img);
            }
            rec
        })
        .collect();

    // Next iterator points past the last record returned.
    let next_seq = records
        .last()
        .and_then(|r| r["dynamodb"]["SequenceNumber"].as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s + 1)
        .unwrap_or(from_seq);

    let next_iterator = format!("{stream_arn}|{shard_id}|{next_seq}");

    Ok(json!({
        "Records": records,
        "NextShardIterator": next_iterator,
    }))
}

/// `ListStreams` — list all streams, optionally filtered by table name.
pub fn list_streams(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_table = opt_str(input, "TableName");
    let limit = input.get("Limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    let streams: Vec<Value> = state
        .tables
        .iter()
        .filter(|entry| {
            let t = entry.value();
            if !t.stream_enabled || t.stream_arn.is_none() {
                return false;
            }
            if let Some(name) = filter_table {
                t.name == name
            } else {
                true
            }
        })
        .take(limit)
        .map(|entry| {
            let t = entry.value();
            let arn = t.stream_arn.as_deref().unwrap_or("");
            json!({
                "StreamArn": arn,
                "StreamLabel": stream_label_from_arn(arn),
                "TableName": t.name,
            })
        })
        .collect();

    Ok(json!({ "Streams": streams }))
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn require_stream_arn(input: &Value) -> Result<&str, AwsError> {
    input
        .get("StreamArn")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::validation("StreamArn is required"))
}

/// Extract the timestamp label from a stream ARN.
///
/// ARN format: `arn:aws:dynamodb:{region}:{account}:table/{name}/stream/{label}`
fn stream_label_from_arn(arn: &str) -> &str {
    arn.rsplit('/').next().unwrap_or(arn)
}

/// Find the table that owns the given stream ARN, returning a DashMap ref.
fn find_table_by_stream_arn<'a>(
    state: &'a DynamoState,
    stream_arn: &str,
) -> Result<dashmap::mapref::one::Ref<'a, String, crate::state::Table>, AwsError> {
    state
        .tables
        .iter()
        .find(|entry| entry.value().stream_arn.as_deref() == Some(stream_arn))
        .and_then(|entry| {
            // Upgrade iter ref to a proper Ref by looking up by key.
            let key = entry.key().clone();
            drop(entry);
            state.tables.get(&key)
        })
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Stream not found: {stream_arn}"),
            )
        })
}
