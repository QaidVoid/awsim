use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::{
    error::resource_not_found,
    executor,
    state::{InvocationRecord, LambdaState},
    util::{new_uuid, now_iso8601, opt_str, require_str},
};

fn map_io_err(e: std::io::Error) -> AwsError {
    AwsError::internal(format!("read function code: {e}"))
}

/// Extract zip bytes to the given directory, returning an error string on failure.
fn extract_zip(zip_bytes: &[u8], dest: &std::path::Path) -> Result<(), String> {
    use std::io::Read;

    std::fs::create_dir_all(dest).map_err(|e| format!("create_dir_all failed: {e}"))?;

    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("zip open failed: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let entry_path = dest.join(entry.name());

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .map_err(|e| format!("mkdir {}: {e}", entry_path.display()))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir parent: {e}"))?;
            }
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|e| format!("read entry: {e}"))?;
            std::fs::write(&entry_path, &data)
                .map_err(|e| format!("write {}: {e}", entry_path.display()))?;
        }
    }

    Ok(())
}

/// Return the cached code directory for a function, extracting the zip if necessary.
/// The cache dir is `{tmp}/awsim-lambda/{function_name}/code/`.
fn ensure_code_dir(
    function_name: &str,
    code_data: &[u8],
    code_sha256: &str,
) -> Result<std::path::PathBuf, String> {
    let cache_dir = std::env::temp_dir()
        .join("awsim-lambda")
        .join(function_name)
        .join("code");

    // Check if already extracted with the same hash
    let stamp_path = cache_dir.join(".awsim_sha256");
    if cache_dir.exists() {
        if let Ok(existing) = std::fs::read_to_string(&stamp_path)
            && existing.trim() == code_sha256
        {
            debug!(function_name, "Using cached code directory");
            return Ok(cache_dir);
        }
        // Hash mismatch — clear and re-extract
        std::fs::remove_dir_all(&cache_dir).map_err(|e| format!("remove stale cache: {e}"))?;
    }

    debug!(function_name, "Extracting zip to cache directory");
    extract_zip(code_data, &cache_dir)?;
    std::fs::write(&stamp_path, code_sha256).map_err(|e| format!("write sha stamp: {e}"))?;

    Ok(cache_dir)
}

pub fn invoke(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let invocation_type = opt_str(input, "InvocationType").unwrap_or("RequestResponse");
    let payload = input.get("Payload").cloned().unwrap_or(json!({}));

    let function_info = {
        let f = state
            .functions
            .get(name)
            .ok_or_else(|| resource_not_found("function", name))?;

        let code_bytes = f
            .code
            .as_ref()
            .map(|c| c.read_all())
            .transpose()
            .map_err(map_io_err)?;

        (
            f.runtime.clone(),
            f.handler.clone(),
            code_bytes,
            f.code_sha256.clone(),
            f.environment.clone(),
            f.timeout,
            f.memory_size,
        )
    };

    let (runtime, handler, code_data, code_sha256, env_vars, timeout, memory_size) = function_info;

    // For DryRun, just validate and return 204 equivalent (no payload)
    if invocation_type == "DryRun" {
        return Ok(json!({ "StatusCode": 204 }));
    }

    let request_id = new_uuid();

    let (response_payload, exec_error) = if let (Some(rt), Some(hndlr), Some(data)) =
        (runtime.as_deref(), handler.as_deref(), code_data.as_deref())
    {
        // Build env vars for the invocation
        let mut invocation_env = env_vars.clone();
        invocation_env
            .entry("AWS_LAMBDA_FUNCTION_NAME".to_string())
            .or_insert_with(|| name.to_string());
        invocation_env
            .entry("AWS_LAMBDA_FUNCTION_MEMORY_SIZE".to_string())
            .or_insert_with(|| memory_size.to_string());
        invocation_env
            .entry("AWS_REQUEST_ID".to_string())
            .or_insert_with(|| request_id.clone());

        let event_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());

        match ensure_code_dir(name, data, &code_sha256) {
            Ok(code_dir) => {
                let result = executor::execute_function(
                    rt,
                    hndlr,
                    &code_dir,
                    &event_json,
                    &invocation_env,
                    timeout,
                );

                let parsed_payload: Value = serde_json::from_str(&result.payload)
                    .unwrap_or(Value::String(result.payload.clone()));

                (parsed_payload, result.error)
            }
            Err(e) => {
                warn!(error = %e, function_name = name, "Failed to extract function code");
                let err_payload = json!({
                    "errorMessage": format!("Failed to prepare function code: {}", e),
                    "errorType": "ServiceException"
                });
                (err_payload, Some("ServiceException".to_string()))
            }
        }
    } else {
        // No code data or missing runtime/handler — fall back to mock
        warn!(
            function_name = name,
            runtime = ?runtime,
            handler = ?handler,
            has_code = code_data.is_some(),
            "Falling back to mock response (no executable code)"
        );
        (json!({ "statusCode": 200, "body": "{}" }), None)
    };

    let status_code: u16 = 200;

    // Record invocation
    let record = InvocationRecord {
        invocation_id: request_id,
        invocation_type: invocation_type.to_string(),
        payload: payload.clone(),
        response: response_payload.clone(),
        status_code,
        timestamp: now_iso8601(),
    };

    if let Some(mut f) = state.functions.get_mut(name) {
        f.invocations.push(record);
        if f.invocations.len() > 1000 {
            f.invocations.remove(0);
        }
    }

    let response_status = if invocation_type == "Event" {
        202u64
    } else {
        200u64
    };

    let mut response = json!({
        "StatusCode": response_status,
        "Payload": response_payload,
        "__status_code": response_status,
    });

    if let Some(err_type) = exec_error {
        // The AWS SDK reads function errors from the X-Amz-Function-Error
        // response header, not from the response body, so surface it both ways.
        response["FunctionError"] = Value::String(err_type.clone());
        response["__headers"] = json!({ "X-Amz-Function-Error": err_type });
    }

    Ok(response)
}
