use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::FirehoseState;

pub fn start_encryption(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required"))?;
    let mut s = state
        .streams
        .get_mut(name)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Stream {name} not found")))?;
    s.encryption_enabled = true;
    s.encryption_key_type = input["DeliveryStreamEncryptionConfigurationInput"]["KeyType"]
        .as_str()
        .map(String::from);
    s.encryption_key_arn = input["DeliveryStreamEncryptionConfigurationInput"]["KeyARN"]
        .as_str()
        .map(String::from);
    Ok(json!({}))
}

pub fn stop_encryption(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required"))?;
    let mut s = state
        .streams
        .get_mut(name)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Stream {name} not found")))?;
    s.encryption_enabled = false;
    s.encryption_key_type = None;
    s.encryption_key_arn = None;
    Ok(json!({}))
}
