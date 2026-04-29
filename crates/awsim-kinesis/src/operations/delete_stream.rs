use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::KinesisState;

pub fn handle(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Accept either StreamName or StreamARN
    let stream_name = resolve_stream_name(state, input)?;

    state.streams.remove(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    if let Some(sqlite) = state.sqlite() {
        let _ = sqlite.delete_stream(&ctx.account_id, &ctx.region, &stream_name);
    }

    info!(stream = %stream_name, "Deleted Kinesis stream");
    Ok(json!({}))
}

pub(crate) fn resolve_stream_name(
    _state: &KinesisState,
    input: &Value,
) -> Result<String, AwsError> {
    if let Some(name) = input["StreamName"].as_str() {
        return Ok(name.to_string());
    }
    if let Some(arn) = input["StreamARN"].as_str() {
        // arn:aws:kinesis:{region}:{account}:stream/{name}
        let name = arn
            .split('/')
            .next_back()
            .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Invalid StreamARN"))?;
        return Ok(name.to_string());
    }
    Err(AwsError::bad_request(
        "MissingParameter",
        "Either StreamName or StreamARN is required",
    ))
}
