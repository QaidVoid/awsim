use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::FirehoseState;

pub fn tag_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    if let Some(tags) = input["Tags"].as_array() {
        for t in tags {
            if let Some(k) = t["Key"].as_str() {
                let v = t["Value"].as_str().unwrap_or("").to_string();
                s.tags.insert(k.to_string(), v);
            }
        }
    }
    Ok(json!({}))
}

pub fn untag_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    if let Some(keys) = input["TagKeys"].as_array() {
        for k in keys {
            if let Some(s_k) = k.as_str() {
                s.tags.remove(s_k);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags_for_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let s = state.streams.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    let tags: Vec<Value> = s
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();
    Ok(json!({
        "Tags": tags,
        "HasMoreTags": false,
    }))
}
