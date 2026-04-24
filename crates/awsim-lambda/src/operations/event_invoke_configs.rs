use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::{EventInvokeConfig, LambdaState},
    util::require_str,
};

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn key(function_name: &str, qualifier: Option<&str>) -> String {
    match qualifier {
        Some(q) => format!("{function_name}:{q}"),
        None => function_name.to_string(),
    }
}

fn qualifier_of<'a>(input: &'a Value) -> Option<&'a str> {
    input.get("Qualifier").and_then(|v| v.as_str())
}

fn destination_of(input: &Value, field: &str) -> Option<String> {
    input
        .get("DestinationConfig")
        .and_then(|d| d.get(field))
        .and_then(|d| d.get("Destination"))
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

fn config_to_json(cfg: &EventInvokeConfig) -> Value {
    let mut destination_config = serde_json::Map::new();
    if let Some(d) = &cfg.destination_on_success {
        destination_config.insert("OnSuccess".to_string(), json!({ "Destination": d }));
    }
    if let Some(d) = &cfg.destination_on_failure {
        destination_config.insert("OnFailure".to_string(), json!({ "Destination": d }));
    }

    json!({
        "FunctionArn": cfg.function_arn,
        "MaximumRetryAttempts": cfg.maximum_retry_attempts,
        "MaximumEventAgeInSeconds": cfg.maximum_event_age_in_seconds,
        "DestinationConfig": Value::Object(destination_config),
        "LastModified": cfg.last_modified,
    })
}

pub fn put_function_event_invoke_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let func = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let k = key(name, qualifier_of(input));
    let cfg = EventInvokeConfig {
        function_arn: func.arn.clone(),
        maximum_retry_attempts: input
            .get("MaximumRetryAttempts")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
        maximum_event_age_in_seconds: input
            .get("MaximumEventAgeInSeconds")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
        destination_on_success: destination_of(input, "OnSuccess"),
        destination_on_failure: destination_of(input, "OnFailure"),
        last_modified: now_epoch(),
    };

    let result = config_to_json(&cfg);
    state.event_invoke_configs.insert(k, cfg);
    Ok(result)
}

pub fn get_function_event_invoke_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let k = key(name, qualifier_of(input));
    let cfg = state
        .event_invoke_configs
        .get(&k)
        .ok_or_else(|| resource_not_found("event-invoke-config", &k))?;
    Ok(config_to_json(&cfg))
}

pub fn update_function_event_invoke_config(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    put_function_event_invoke_config(state, input, ctx)
}

pub fn delete_function_event_invoke_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let k = key(name, qualifier_of(input));
    state.event_invoke_configs.remove(&k);
    Ok(json!({}))
}

pub fn list_function_event_invoke_configs(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let prefix = format!("{name}");
    let configs: Vec<Value> = state
        .event_invoke_configs
        .iter()
        .filter(|e| e.key() == &prefix || e.key().starts_with(&format!("{prefix}:")))
        .map(|e| config_to_json(e.value()))
        .collect();
    Ok(json!({ "FunctionEventInvokeConfigs": configs }))
}
