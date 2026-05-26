use awsim_core::pagination::{cap_max_results, paginate};
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

    // AWS caps MaxResults at 10_000 with a default of 100.
    let max_results = cap_max_results(input["MaxResults"].as_i64(), 100, 10_000);
    let next_token = input["NextToken"].as_str();

    let mut shards = stream.shards.clone();
    drop(stream);
    // Stable ordering — AWS doesn't promise a particular ordering but
    // pagination requires it.
    shards.sort_by(|a, b| a.shard_id.cmp(&b.shard_id));

    let page = paginate(shards, max_results, next_token, |s| s.shard_id.clone())?;

    let shards: Vec<Value> = page
        .items
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

    let mut response = json!({ "Shards": shards });
    if let Some(token) = page.next_token {
        response["NextToken"] = json!(token);
    } else {
        response["NextToken"] = Value::Null;
    }
    Ok(response)
}
