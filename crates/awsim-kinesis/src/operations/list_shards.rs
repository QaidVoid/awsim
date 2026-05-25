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

    let shards: Vec<Value> = stream
        .shards
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
        "Shards": shards,
        "NextToken": Value::Null,
    }))
}
