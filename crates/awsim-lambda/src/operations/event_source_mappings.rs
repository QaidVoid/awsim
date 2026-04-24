use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::{EventSourceMapping, LambdaState},
    util::{new_uuid, now_iso8601, opt_bool, opt_str, opt_u64, require_str},
};

fn mapping_to_value(m: &EventSourceMapping) -> Value {
    json!({
        "UUID": m.uuid,
        "EventSourceArn": m.event_source_arn,
        "FunctionArn": m.function_arn,
        "BatchSize": m.batch_size,
        "State": m.state,
        "StateTransitionReason": "USER_INITIATED",
        "LastModified": m.last_modified,
    })
}

pub fn create_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let event_source_arn = require_str(input, "EventSourceArn")?;
    let function_name = require_str(input, "FunctionName")?;
    let batch_size = opt_u64(input, "BatchSize").unwrap_or(10) as u32;
    let enabled = opt_bool(input, "Enabled").unwrap_or(true);

    // Resolve function ARN
    let function_arn = if function_name.starts_with("arn:") {
        function_name.to_string()
    } else {
        // Look up the function to get its ARN
        match state.functions.get(function_name) {
            Some(f) => f.arn.clone(),
            None => format!(
                "arn:aws:lambda:{}:{}:function:{}",
                ctx.region, ctx.account_id, function_name
            ),
        }
    };

    let uuid = new_uuid();
    let mapping = EventSourceMapping {
        uuid: uuid.clone(),
        event_source_arn: event_source_arn.to_string(),
        function_arn,
        batch_size,
        enabled,
        state: if enabled {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        },
        last_modified: now_iso8601(),
    };

    let result = mapping_to_value(&mapping);
    state.event_source_mappings.insert(uuid, mapping);

    Ok(result)
}

pub fn get_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let uuid = require_str(input, "UUID")?;
    let m = state
        .event_source_mappings
        .get(uuid)
        .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
    Ok(mapping_to_value(&m))
}

pub fn delete_event_source_mapping(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let uuid = require_str(input, "UUID")?;
    let (_, m) = state
        .event_source_mappings
        .remove(uuid)
        .ok_or_else(|| resource_not_found("event source mapping", uuid))?;
    Ok(mapping_to_value(&m))
}

pub fn list_event_source_mappings(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_source_arn = opt_str(input, "EventSourceArn");
    let filter_function = opt_str(input, "FunctionName");

    let mappings: Vec<Value> = state
        .event_source_mappings
        .iter()
        .filter(|m| {
            if let Some(arn) = filter_source_arn
                && !m.event_source_arn.contains(arn) {
                    return false;
                }
            if let Some(fn_name) = filter_function
                && !m.function_arn.contains(fn_name) {
                    return false;
                }
            true
        })
        .map(|m| mapping_to_value(&m))
        .collect();

    Ok(json!({ "EventSourceMappings": mappings }))
}
