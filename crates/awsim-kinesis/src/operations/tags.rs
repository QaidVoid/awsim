use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::KinesisState;
use super::delete_stream::resolve_stream_name;

pub fn add_tags(state: &KinesisState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    if let Some(tags) = input["Tags"].as_object() {
        for (k, v) in tags {
            if let Some(s) = v.as_str() {
                stream.tags.insert(k.clone(), s.to_string());
            }
        }
    }

    Ok(json!({}))
}

pub fn remove_tags(state: &KinesisState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;

    let mut stream = state.streams.get_mut(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    if let Some(keys) = input["TagKeys"].as_array() {
        for key in keys {
            if let Some(k) = key.as_str() {
                stream.tags.remove(k);
            }
        }
    }

    Ok(json!({}))
}

pub fn list_tags(state: &KinesisState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let stream_name = resolve_stream_name(state, input)?;

    let stream = state.streams.get(&stream_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {} does not exist", stream_name),
        )
    })?;

    let tags: Vec<Value> = stream
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({
        "Tags": tags,
        "HasMoreTags": false,
    }))
}
