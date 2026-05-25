use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, ShardIteratorInfo};
use crate::util::encode_iterator;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
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
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    let shard_index = stream
        .shards
        .iter()
        .position(|s| s.shard_id == shard_id)
        .ok_or_else(|| {
            AwsError::bad_request(
                "ResourceNotFoundException",
                format!("Shard {shard_id} does not exist in stream {stream_name}"),
            )
        })?;

    let sqlite = state
        .sqlite()
        .ok_or_else(|| AwsError::internal("Kinesis sqlite store not initialised"))?;

    // Iterator semantics — `position` is the exclusive lower bound;
    // `GetRecords` returns rows with `seq > position`.
    //
    // Sequence numbers are 1-based, so `position = 0` means
    // "from the start". `LATEST` skips ahead to the highest stored
    // seq so only future records are visible.
    let position: u64 = match iterator_type {
        "TRIM_HORIZON" => 0,
        "LATEST" => sqlite
            .max_seq(&ctx.account_id, &ctx.region, stream_name, shard_id)
            .unwrap_or(0) as u64,
        "AT_SEQUENCE_NUMBER" => {
            let seq_str = input["StartingSequenceNumber"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "MissingParameter",
                    "StartingSequenceNumber is required for AT_SEQUENCE_NUMBER",
                )
            })?;
            // `AT` means inclusive — set the cursor one before the
            // requested seq so the requested record is the next read.
            let seq: u64 = seq_str.parse().unwrap_or(0);
            seq.saturating_sub(1)
        }
        "AFTER_SEQUENCE_NUMBER" => {
            let seq_str = input["StartingSequenceNumber"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "MissingParameter",
                    "StartingSequenceNumber is required for AFTER_SEQUENCE_NUMBER",
                )
            })?;
            seq_str.parse().unwrap_or(0)
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("Unknown ShardIteratorType: {other}"),
            ));
        }
    };

    let info = ShardIteratorInfo {
        stream_name: stream_name.to_string(),
        shard_index,
        position,
    };

    let token = encode_iterator(&info);
    state.iterators.insert(token.clone(), info);

    Ok(json!({
        "ShardIterator": token,
    }))
}
