use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::{FunctionUrlConfig, LambdaState},
    util::{now_iso8601, opt_str, require_str},
};

// ---------------------------------------------------------------------------
// GetFunctionUrlConfig
// ---------------------------------------------------------------------------

pub fn get_function_url_config(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    // Verify function exists
    state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let cfg = state.url_configs.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Function URL config not found for function: {name}"),
        )
    })?;

    Ok(url_config_to_value(&cfg, ctx))
}

// ---------------------------------------------------------------------------
// CreateFunctionUrlConfig
// ---------------------------------------------------------------------------

pub fn create_function_url_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    // Verify function exists
    let func_arn = {
        let f = state
            .functions
            .get(name)
            .ok_or_else(|| resource_not_found("function", name))?;
        f.arn.clone()
    };

    if state.url_configs.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceConflictException",
            format!("Function URL config already exists for function: {name}"),
        ));
    }

    let auth_type = opt_str(input, "AuthType")
        .unwrap_or("NONE")
        .to_string();
    let cors = input.get("Cors").cloned();
    let now = now_iso8601();
    let function_url = format!(
        "http://localhost:4566/lambda/{}",
        name
    );

    let creation_time = now.clone();
    let last_modified_time = now.clone();

    let cfg = FunctionUrlConfig {
        function_name: name.to_string(),
        function_arn: func_arn.clone(),
        function_url: function_url.clone(),
        auth_type: auth_type.clone(),
        cors: cors.clone(),
        creation_time: creation_time.clone(),
        last_modified_time: last_modified_time.clone(),
    };

    state.url_configs.insert(name.to_string(), cfg);

    Ok(json!({
        "FunctionUrl": function_url,
        "FunctionArn": func_arn,
        "AuthType": auth_type,
        "Cors": cors,
        "CreationTime": creation_time,
        "LastModifiedTime": last_modified_time,
    }))
}

// ---------------------------------------------------------------------------
// DeleteFunctionUrlConfig
// ---------------------------------------------------------------------------

pub fn delete_function_url_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    state.url_configs.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Function URL config not found for function: {name}"),
        )
    })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListFunctionUrlConfigs
// ---------------------------------------------------------------------------

pub fn list_function_url_configs(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    // Verify function exists
    state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let configs: Vec<Value> = if let Some(cfg) = state.url_configs.get(name) {
        vec![url_config_to_value(&cfg, ctx)]
    } else {
        vec![]
    };

    Ok(json!({ "FunctionUrlConfigs": configs }))
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn url_config_to_value(cfg: &FunctionUrlConfig, _ctx: &RequestContext) -> Value {
    json!({
        "FunctionUrl": cfg.function_url,
        "FunctionArn": cfg.function_arn,
        "AuthType": cfg.auth_type,
        "Cors": cfg.cors,
        "CreationTime": cfg.creation_time,
        "LastModifiedTime": cfg.last_modified_time,
    })
}
