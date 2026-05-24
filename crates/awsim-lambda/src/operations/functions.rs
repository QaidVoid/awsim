use awsim_core::{AwsError, Body, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{invalid_parameter, resource_conflict, resource_not_found},
    state::{LambdaFunction, LambdaState},
    util::{
        decode_zip, now_iso8601, opt_str, require_str, sha256_base64, validate_handler,
        validate_runtime,
    },
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

    let mut out = json!({
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
        // SDK waiters (wait_until_function_updated_v2,
        // wait_until_function_active_v2) poll these two fields and only
        // stop once both reach a terminal value. Our handlers run
        // synchronously, so every visible state is terminal Successful;
        // exposing the field is what unblocks the waiter loop.
        "LastUpdateStatus": "Successful",
        "LastUpdateStatusReason": Value::Null,
        "LastUpdateStatusReasonCode": Value::Null,
        "Architectures": f.architectures,
        "EphemeralStorage": { "Size": f.ephemeral_storage_size },
        "PackageType": f.package_type,
        // TracingConfig defaults to PassThrough when unset; AWS always
        // emits the field on the response, even for new functions that
        // never set it.
        "TracingConfig": f.tracing_config.clone()
            .unwrap_or_else(|| json!({ "Mode": "PassThrough" })),
    });
    let obj = out.as_object_mut().expect("object");
    if !f.layers.is_empty() {
        // ListFunctions surfaces layers as `[{ Arn, CodeSize, SigningProfileVersionArn, SigningJobArn }]`.
        // We only round-trip the ARN since the rest of those metadata fields
        // come from the layer-version record and aren't worth synthesizing.
        let layers: Vec<Value> = f.layers.iter().map(|arn| json!({ "Arn": arn })).collect();
        obj.insert("Layers".into(), Value::Array(layers));
    }
    if let Some(v) = &f.vpc_config {
        obj.insert("VpcConfig".into(), v.clone());
    }
    if let Some(v) = &f.dead_letter_config {
        obj.insert("DeadLetterConfig".into(), v.clone());
    }
    if let Some(arn) = &f.kms_key_arn {
        obj.insert("KMSKeyArn".into(), Value::String(arn.clone()));
    }
    if let Some(v) = &f.file_system_configs {
        obj.insert("FileSystemConfigs".into(), v.clone());
    }
    if let Some(v) = &f.logging_config {
        obj.insert("LoggingConfig".into(), v.clone());
    }
    if let Some(v) = &f.snap_start {
        // SDK reads SnapStart.OptimizationStatus alongside ApplyOn; default
        // to Off for ApplyOn=None and On for PublishedVersions.
        let mut snap = v.clone();
        if let Some(o) = snap.as_object_mut() {
            let apply_on = o
                .get("ApplyOn")
                .and_then(|v| v.as_str())
                .unwrap_or("None")
                .to_string();
            let opt = if apply_on == "PublishedVersions" {
                "On"
            } else {
                "Off"
            };
            o.entry("OptimizationStatus")
                .or_insert(Value::String(opt.to_string()));
        }
        obj.insert("SnapStart".into(), snap);
    }
    if let Some(v) = &f.image_config {
        obj.insert("ImageConfigResponse".into(), json!({ "ImageConfig": v }));
    }
    out
}

/// Validate Architectures: AWS allows at most one entry of "x86_64" or "arm64".
fn parse_architectures(input: &Value) -> Result<Option<Vec<String>>, AwsError> {
    let Some(arr) = input.get("Architectures").and_then(|v| v.as_array()) else {
        return Ok(None);
    };
    if arr.len() != 1 {
        return Err(invalid_parameter(
            "Architectures must contain exactly one value",
        ));
    }
    let arch = arr[0]
        .as_str()
        .ok_or_else(|| invalid_parameter("Architectures entries must be strings"))?;
    if arch != "x86_64" && arch != "arm64" {
        return Err(invalid_parameter(
            "Architectures must be one of: x86_64, arm64",
        ));
    }
    Ok(Some(vec![arch.to_string()]))
}

/// Validate EphemeralStorage: AWS allows Size in [512, 10240] MiB.
fn parse_ephemeral_storage_size(input: &Value) -> Result<Option<u32>, AwsError> {
    let Some(size) = input
        .get("EphemeralStorage")
        .and_then(|v| v.get("Size"))
        .and_then(|v| v.as_u64())
    else {
        return Ok(None);
    };
    if !(512..=10240).contains(&size) {
        return Err(invalid_parameter(
            "EphemeralStorage.Size must be between 512 and 10240",
        ));
    }
    Ok(Some(size as u32))
}

/// Validate TracingConfig: Mode is one of Active | PassThrough.
fn validate_tracing_config(input: &Value) -> Result<Option<Value>, AwsError> {
    let Some(tracing) = input.get("TracingConfig").cloned() else {
        return Ok(None);
    };
    if !tracing.is_object() {
        return Err(invalid_parameter(
            "TracingConfig must be an object with a Mode field",
        ));
    }
    let mode = tracing
        .get("Mode")
        .and_then(|v| v.as_str())
        .unwrap_or("PassThrough");
    if mode != "Active" && mode != "PassThrough" {
        return Err(invalid_parameter(
            "TracingConfig.Mode must be Active or PassThrough",
        ));
    }
    Ok(Some(tracing))
}

/// Validate SnapStart: ApplyOn is one of None | PublishedVersions.
fn validate_snap_start(input: &Value) -> Result<Option<Value>, AwsError> {
    let Some(snap) = input.get("SnapStart").cloned() else {
        return Ok(None);
    };
    let apply_on = snap
        .get("ApplyOn")
        .and_then(|v| v.as_str())
        .unwrap_or("None");
    if apply_on != "None" && apply_on != "PublishedVersions" {
        return Err(invalid_parameter(
            "SnapStart.ApplyOn must be None or PublishedVersions",
        ));
    }
    Ok(Some(snap))
}

fn parse_layers(input: &Value) -> Vec<String> {
    input
        .get("Layers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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

pub(crate) fn persist_code(
    state: &LambdaState,
    function_name: &str,
    key: &str,
    bytes: Option<Vec<u8>>,
) -> Result<Option<Body>, AwsError> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    match state.body_store() {
        Some(store) => {
            let path = store
                .write_blob("lambda", function_name, key, &bytes)
                .map_err(|e| AwsError::internal(format!("persist function code: {e}")))?;
            Ok(Some(Body::OnDisk(path)))
        }
        None => Ok(Some(Body::InMemory(bytes))),
    }
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
    if let Some(r) = runtime.as_deref() {
        validate_runtime(r)?;
    }
    if let Some(h) = handler.as_deref() {
        validate_handler(h)?;
    }
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
    let code = persist_code(state, name, "$LATEST", code_data)?;

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

    let architectures = parse_architectures(input)?.unwrap_or_else(|| vec!["x86_64".to_string()]);
    let ephemeral_storage_size = parse_ephemeral_storage_size(input)?.unwrap_or(512);
    let snap_start = validate_snap_start(input)?;
    let package_type = opt_str(input, "PackageType").unwrap_or("Zip").to_string();
    let layers = parse_layers(input);

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
        code,
        environment,
        version: "$LATEST".to_string(),
        versions: vec![],
        aliases: HashMap::new(),
        last_modified: now,
        state: "Active".to_string(),
        invocations: vec![],
        policy_statements: HashMap::new(),
        tags,
        reserved_concurrent_executions: None,
        provisioned_concurrency: HashMap::new(),
        architectures,
        ephemeral_storage_size,
        package_type,
        layers,
        vpc_config: input.get("VpcConfig").cloned(),
        dead_letter_config: input.get("DeadLetterConfig").cloned(),
        tracing_config: validate_tracing_config(input)?,
        kms_key_arn: opt_str(input, "KMSKeyArn").map(str::to_string),
        file_system_configs: input.get("FileSystemConfigs").cloned(),
        logging_config: input.get("LoggingConfig").cloned(),
        snap_start,
        image_config: input.get("ImageConfig").cloned(),
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
    if let Some(store) = state.body_store()
        && let Err(e) = store.delete_bucket("lambda", name)
    {
        tracing::warn!(function_name = name, error = %e, "delete persisted function code");
    }
    Ok(json!({}))
}

pub fn list_functions(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let mut all: Vec<LambdaFunction> = state.functions.iter().map(|f| f.value().clone()).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 50, 50);
    let marker = input.get("Marker").and_then(Value::as_str);
    let page = paginate(all, max, marker, |f| f.name.clone())?;

    let functions: Vec<Value> = page.items.iter().map(function_configuration).collect();
    let mut result = json!({ "Functions": functions });
    if let Some(token) = page.next_token {
        result["NextMarker"] = json!(token);
    }
    Ok(result)
}

pub fn update_function_code(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    let (code_data, code_sha256, code_size) = resolve_code(input)?;
    let code = persist_code(state, name, "$LATEST", code_data)?;

    let mut f = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    let cache_dir = std::env::temp_dir()
        .join("awsim-lambda")
        .join(name)
        .join("code");
    if cache_dir.exists() {
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    f.code = code;
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
        validate_handler(handler)?;
        f.handler = Some(handler.to_string());
    }
    if let Some(runtime) = opt_str(input, "Runtime") {
        validate_runtime(runtime)?;
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
    if let Some(arch) = parse_architectures(input)? {
        f.architectures = arch;
    }
    if let Some(size) = parse_ephemeral_storage_size(input)? {
        f.ephemeral_storage_size = size;
    }
    if let Some(snap) = validate_snap_start(input)? {
        f.snap_start = Some(snap);
    }
    if input.get("VpcConfig").is_some() {
        f.vpc_config = input.get("VpcConfig").cloned();
    }
    if input.get("DeadLetterConfig").is_some() {
        f.dead_letter_config = input.get("DeadLetterConfig").cloned();
    }
    if let Some(tracing) = validate_tracing_config(input)? {
        f.tracing_config = Some(tracing);
    }
    if let Some(arn) = opt_str(input, "KMSKeyArn") {
        f.kms_key_arn = Some(arn.to_string());
    }
    if input.get("FileSystemConfigs").is_some() {
        f.file_system_configs = input.get("FileSystemConfigs").cloned();
    }
    if input.get("LoggingConfig").is_some() {
        f.logging_config = input.get("LoggingConfig").cloned();
    }
    if input.get("ImageConfig").is_some() {
        f.image_config = input.get("ImageConfig").cloned();
    }
    if input.get("Layers").is_some() {
        f.layers = parse_layers(input);
    }
    f.last_modified = now_iso8601();

    Ok(function_configuration(&f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn empty_zip_b64() -> String {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        let bytes: [u8; 22] = [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        BASE64.encode(bytes)
    }

    fn create_input(runtime: &str, handler: &str) -> Value {
        json!({
            "FunctionName": "f",
            "Role": "arn:aws:iam::000000000000:role/test",
            "Runtime": runtime,
            "Handler": handler,
            "Code": { "ZipFile": empty_zip_b64() },
        })
    }

    #[test]
    fn create_function_rejects_unknown_runtime() {
        let state = LambdaState::default();
        let err = create_function(&state, &create_input("ferris1.x", "index.handler"), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
        assert!(err.message.contains("ferris1.x"));
    }

    #[test]
    fn create_function_rejects_handler_with_whitespace() {
        let state = LambdaState::default();
        let err = create_function(
            &state,
            &create_input("nodejs20.x", "index .handler"),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
        assert!(err.message.contains("whitespace"));
    }

    #[test]
    fn create_function_rejects_empty_handler() {
        let state = LambdaState::default();
        let err = create_function(&state, &create_input("nodejs20.x", ""), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_function_accepts_valid_runtime_and_handler() {
        let state = LambdaState::default();
        let resp =
            create_function(&state, &create_input("python3.12", "app.handler"), &ctx()).unwrap();
        assert_eq!(resp["Runtime"], json!("python3.12"));
        assert_eq!(resp["Handler"], json!("app.handler"));
    }

    #[test]
    fn create_function_emits_default_architectures_and_ephemeral_storage() {
        let state = LambdaState::default();
        let resp =
            create_function(&state, &create_input("nodejs20.x", "index.handler"), &ctx()).unwrap();
        assert_eq!(resp["Architectures"], json!(["x86_64"]));
        assert_eq!(resp["EphemeralStorage"]["Size"], json!(512));
        assert_eq!(resp["PackageType"], json!("Zip"));
        assert_eq!(resp["TracingConfig"]["Mode"], json!("PassThrough"));
    }

    #[test]
    fn create_function_round_trips_architectures_and_ephemeral_storage() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["Architectures"] = json!(["arm64"]);
        input["EphemeralStorage"] = json!({ "Size": 2048 });
        let resp = create_function(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["Architectures"], json!(["arm64"]));
        assert_eq!(resp["EphemeralStorage"]["Size"], json!(2048));
    }

    #[test]
    fn create_function_rejects_invalid_architecture() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["Architectures"] = json!(["mips"]);
        let err = create_function(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_function_rejects_ephemeral_storage_out_of_range() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["EphemeralStorage"] = json!({ "Size": 256 });
        let err = create_function(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");

        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["EphemeralStorage"] = json!({ "Size": 20480 });
        let err = create_function(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_function_round_trips_vpc_and_dead_letter_and_tracing() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["VpcConfig"] = json!({
            "SubnetIds": ["subnet-1"],
            "SecurityGroupIds": ["sg-1"],
        });
        input["DeadLetterConfig"] =
            json!({ "TargetArn": "arn:aws:sqs:us-east-1:000000000000:dlq" });
        input["TracingConfig"] = json!({ "Mode": "Active" });
        input["KMSKeyArn"] = json!("arn:aws:kms:us-east-1:000000000000:key/abc");
        input["Layers"] = json!(["arn:aws:lambda:us-east-1:000000000000:layer:shared:1",]);
        let resp = create_function(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["VpcConfig"]["SubnetIds"], json!(["subnet-1"]));
        assert_eq!(
            resp["DeadLetterConfig"]["TargetArn"],
            json!("arn:aws:sqs:us-east-1:000000000000:dlq")
        );
        assert_eq!(resp["TracingConfig"]["Mode"], json!("Active"));
        assert_eq!(
            resp["KMSKeyArn"],
            json!("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
        assert_eq!(
            resp["Layers"][0]["Arn"],
            json!("arn:aws:lambda:us-east-1:000000000000:layer:shared:1")
        );
    }

    #[test]
    fn create_function_snap_start_published_versions_emits_optimization_status_on() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["SnapStart"] = json!({ "ApplyOn": "PublishedVersions" });
        let resp = create_function(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["SnapStart"]["ApplyOn"], json!("PublishedVersions"));
        assert_eq!(resp["SnapStart"]["OptimizationStatus"], json!("On"));
    }

    #[test]
    fn create_function_rejects_invalid_snap_start_apply_on() {
        let state = LambdaState::default();
        let mut input = create_input("nodejs20.x", "index.handler");
        input["SnapStart"] = json!({ "ApplyOn": "Always" });
        let err = create_function(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn update_function_configuration_can_change_architecture_and_ephemeral_storage() {
        let state = LambdaState::default();
        create_function(&state, &create_input("nodejs20.x", "index.handler"), &ctx()).unwrap();
        let resp = update_function_configuration(
            &state,
            &json!({
                "FunctionName": "f",
                "Architectures": ["arm64"],
                "EphemeralStorage": { "Size": 1024 },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Architectures"], json!(["arm64"]));
        assert_eq!(resp["EphemeralStorage"]["Size"], json!(1024));
    }

    #[test]
    fn create_function_omits_runtime_validation_when_runtime_absent() {
        // Container-image (PackageType=Image) functions don't carry a
        // Runtime — must accept the absent case.
        let state = LambdaState::default();
        let resp = create_function(
            &state,
            &json!({
                "FunctionName": "f",
                "Role": "arn:aws:iam::000000000000:role/test",
                "Code": { "ZipFile": empty_zip_b64() },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["FunctionName"], json!("f"));
    }
}
