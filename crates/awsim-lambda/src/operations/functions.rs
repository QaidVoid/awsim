use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{invalid_parameter, resource_conflict, resource_not_found},
    state::{LambdaFunction, LambdaState},
    util::{decode_zip, now_iso8601, opt_str, require_str, sha256_base64},
};

/// Serialize a LambdaFunction into its FunctionConfiguration JSON shape.
pub fn function_configuration(f: &LambdaFunction) -> Value {
    let env = if f.environment.is_empty() {
        json!(null)
    } else {
        let vars: serde_json::Map<String, Value> = f
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        json!({ "Variables": vars })
    };

    json!({
        "FunctionName": f.name,
        "FunctionArn": f.arn,
        "Runtime": f.runtime,
        "Role": f.role,
        "Handler": f.handler,
        "Description": f.description,
        "Timeout": f.timeout,
        "MemorySize": f.memory_size,
        "CodeSha256": f.code_sha256,
        "CodeSize": f.code_size,
        "Environment": env,
        "Version": f.version,
        "LastModified": f.last_modified,
        "State": f.state,
        "FunctionArn": f.arn,
    })
}

/// Resolve code bytes (either from ZipFile base64 or a placeholder for S3).
/// Returns (bytes_opt, sha256, size).
fn resolve_code(input: &Value) -> Result<(Option<Vec<u8>>, String, u64), AwsError> {
    if let Some(zip_b64) = opt_str(input, "ZipFile") {
        let (bytes, hash, size) = decode_zip(zip_b64)?;
        return Ok((Some(bytes), hash, size));
    }

    // Try nested Code object
    if let Some(code) = input.get("Code") {
        if let Some(zip_b64) = opt_str(code, "ZipFile") {
            let (bytes, hash, size) = decode_zip(zip_b64)?;
            return Ok((Some(bytes), hash, size));
        }
        if opt_str(code, "S3Bucket").is_some() {
            // S3 source — we don't actually fetch it; return a placeholder
            let placeholder = b"s3-placeholder";
            let hash = sha256_base64(placeholder);
            return Ok((None, hash, 0));
        }
    }

    // Try top-level S3 params
    if opt_str(input, "S3Bucket").is_some() {
        let placeholder = b"s3-placeholder";
        let hash = sha256_base64(placeholder);
        return Ok((None, hash, 0));
    }

    Err(invalid_parameter(
        "Either ZipFile or S3Bucket/S3Key must be provided in Code",
    ))
}

pub fn create_function(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    if state.functions.contains_key(name) {
        return Err(resource_conflict(format!("Function already exist: {name}")));
    }

    let role = require_str(input, "Role")?;
    let runtime = opt_str(input, "Runtime").map(str::to_string);
    let handler = opt_str(input, "Handler").map(str::to_string);
    let description = opt_str(input, "Description").unwrap_or("").to_string();
    let timeout = input.get("Timeout").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
    let memory_size = input
        .get("MemorySize")
        .and_then(|v| v.as_u64())
        .unwrap_or(128) as u32;

    let environment: HashMap<String, String> = input
        .get("Environment")
        .and_then(|e| e.get("Variables"))
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let (code_data, code_sha256, code_size) = resolve_code(input)?;

    let arn = format!(
        "arn:aws:lambda:{}:{}:function:{}",
        ctx.region, ctx.account_id, name
    );
    let now = now_iso8601();

    // Extract tags from CreateFunction input
    let tags: HashMap<String, String> = input
        .get("Tags")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let func = LambdaFunction {
        name: name.to_string(),
        arn: arn.clone(),
        runtime,
        role: role.to_string(),
        handler,
        description,
        timeout,
        memory_size,
        code_sha256,
        code_size,
        code_data,
        environment,
        version: "$LATEST".to_string(),
        versions: vec![],
        aliases: HashMap::new(),
        last_modified: now,
        state: "Active".to_string(),
        invocations: vec![],
        policy_statements: HashMap::new(),
        tags,
    };

    let config = function_configuration(&func);
    state.functions.insert(name.to_string(), func);

    Ok(config)
}

pub fn get_function(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let f = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let config = function_configuration(&f);
    let code_location = format!("http://awsim.local/2015-03-31/functions/{}/code", f.name);

    Ok(json!({
        "Configuration": config,
        "Code": {
            "Location": code_location,
            "RepositoryType": "S3",
        },
    }))
}

pub fn get_function_configuration(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let f = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    Ok(function_configuration(&f))
}

pub fn delete_function(state: &LambdaState, input: &Value) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    state
        .functions
        .remove(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    Ok(json!({}))
}

pub fn list_functions(
    state: &LambdaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let functions: Vec<Value> = state
        .functions
        .iter()
        .map(|f| function_configuration(&f))
        .collect();

    Ok(json!({ "Functions": functions }))
}

pub fn update_function_code(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    let (code_data, code_sha256, code_size) = resolve_code(input)?;

    let mut f = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    // Invalidate the on-disk code cache so the next invocation re-extracts.
    let cache_dir = std::env::temp_dir()
        .join("awsim-lambda")
        .join(name)
        .join("code");
    if cache_dir.exists() {
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    f.code_data = code_data;
    f.code_sha256 = code_sha256;
    f.code_size = code_size;
    f.last_modified = now_iso8601();

    Ok(function_configuration(&f))
}

pub fn update_function_configuration(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    let mut f = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    if let Some(role) = opt_str(input, "Role") {
        f.role = role.to_string();
    }
    if let Some(handler) = opt_str(input, "Handler") {
        f.handler = Some(handler.to_string());
    }
    if let Some(runtime) = opt_str(input, "Runtime") {
        f.runtime = Some(runtime.to_string());
    }
    if let Some(desc) = opt_str(input, "Description") {
        f.description = desc.to_string();
    }
    if let Some(timeout) = input.get("Timeout").and_then(|v| v.as_u64()) {
        f.timeout = timeout as u32;
    }
    if let Some(mem) = input.get("MemorySize").and_then(|v| v.as_u64()) {
        f.memory_size = mem as u32;
    }
    if let Some(env) = input.get("Environment")
        && let Some(vars) = env.get("Variables").and_then(|v| v.as_object())
    {
        f.environment = vars
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
    }
    f.last_modified = now_iso8601();

    Ok(function_configuration(&f))
}
