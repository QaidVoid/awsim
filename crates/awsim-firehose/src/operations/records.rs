use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::FirehoseState;

pub fn put_record(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required"))?;
    if !state.streams.contains_key(name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        ));
    }
    let record_id = uuid::Uuid::new_v4().to_string();
    Ok(json!({
        "RecordId": record_id,
        "Encrypted": false,
    }))
}

pub fn put_record_batch(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required"))?;
    if !state.streams.contains_key(name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        ));
    }
    let count = input["Records"].as_array().map(|a| a.len()).unwrap_or(0);
    let entries: Vec<Value> = (0..count)
        .map(|_| json!({ "RecordId": uuid::Uuid::new_v4().to_string() }))
        .collect();
    Ok(json!({
        "FailedPutCount": 0,
        "Encrypted": false,
        "RequestResponses": entries,
    }))
}
