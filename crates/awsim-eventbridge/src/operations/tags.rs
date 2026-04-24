use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EventBridgeState;

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "ResourceARN is required"))?;

    let tag_list = input["Tags"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Tags is required"))?;

    // Determine if the ARN refers to a bus or a rule
    let mut bus_mut = find_bus_by_arn_mut(state, resource_arn)?;

    for tag in tag_list {
        if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
            bus_mut.tags.insert(k.to_string(), v.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "ResourceARN is required"))?;

    let tag_keys = input["TagKeys"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "TagKeys is required"))?;

    let mut bus_mut = find_bus_by_arn_mut(state, resource_arn)?;

    for key in tag_keys {
        if let Some(k) = key.as_str() {
            bus_mut.tags.remove(k);
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTagsForResource
// ---------------------------------------------------------------------------

pub fn list_tags_for_resource(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "ResourceARN is required"))?;

    let bus = find_bus_by_arn(state, resource_arn)?;

    let tags: Vec<Value> = bus
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();

    Ok(json!({ "Tags": tags }))
}

// ---------------------------------------------------------------------------
// Helpers — find an event bus by its ARN
// ---------------------------------------------------------------------------

fn find_bus_by_arn<'a>(
    state: &'a EventBridgeState,
    arn: &str,
) -> Result<dashmap::mapref::one::Ref<'a, String, crate::state::EventBus>, AwsError> {
    // ARN format: arn:aws:events:{region}:{account}:event-bus/{name}
    let bus_name = extract_bus_name_from_arn(arn)?;

    state.event_buses.get(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Resource {arn} does not exist"),
        )
    })
}

fn find_bus_by_arn_mut<'a>(
    state: &'a EventBridgeState,
    arn: &str,
) -> Result<dashmap::mapref::one::RefMut<'a, String, crate::state::EventBus>, AwsError> {
    let bus_name = extract_bus_name_from_arn(arn)?;

    state.event_buses.get_mut(bus_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Resource {arn} does not exist"),
        )
    })
}

fn extract_bus_name_from_arn(arn: &str) -> Result<&str, AwsError> {
    // Expect "arn:aws:events:*:*:event-bus/{name}"
    arn.split("event-bus/").nth(1).ok_or_else(|| {
        AwsError::bad_request("InvalidParameterValue", format!("Cannot parse ARN: {arn}"))
    })
}
