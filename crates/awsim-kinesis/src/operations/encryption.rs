use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;

/// StartStreamEncryption — enable KMS encryption on a stream.
pub fn start_stream_encryption(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;
    let encryption_type = input["EncryptionType"]
        .as_str()
        .unwrap_or("KMS");
    let key_id = input["KeyId"].as_str().unwrap_or("").to_string();

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    stream.encryption_type = encryption_type.to_string();
    stream.key_id = if key_id.is_empty() { None } else { Some(key_id) };

    Ok(json!({}))
}

/// StopStreamEncryption — disable encryption on a stream.
pub fn stop_stream_encryption(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_name = input["StreamName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamName is required"))?;

    let mut stream = state.streams.get_mut(stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    stream.encryption_type = "NONE".to_string();
    stream.key_id = None;

    Ok(json!({}))
}
