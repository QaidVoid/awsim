use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::{InvocationRecord, LambdaState},
    util::{now_iso8601, opt_str, require_str, new_uuid},
};

pub fn invoke(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let invocation_type = opt_str(input, "InvocationType").unwrap_or("RequestResponse");
    let payload = input.get("Payload").cloned().unwrap_or(json!({}));

    // Verify function exists
    {
        let _ = state
            .functions
            .get(name)
            .ok_or_else(|| resource_not_found("function", name))?;
    }

    // For DryRun, just validate and return 204 equivalent (no payload)
    if invocation_type == "DryRun" {
        return Ok(json!({ "StatusCode": 204 }));
    }

    // Mock response — real execution comes later
    let mock_response = json!({ "statusCode": 200, "body": "{}" });
    let status_code: u16 = 200;

    // Record invocation for debugging
    let record = InvocationRecord {
        invocation_id: new_uuid(),
        invocation_type: invocation_type.to_string(),
        payload: payload.clone(),
        response: mock_response.clone(),
        status_code,
        timestamp: now_iso8601(),
    };

    if let Some(mut f) = state.functions.get_mut(name) {
        f.invocations.push(record);
        // Keep at most 1000 invocation records
        if f.invocations.len() > 1000 {
            f.invocations.remove(0);
        }
    }

    // Event (async) invocations return 202
    let response_status = if invocation_type == "Event" { 202u64 } else { 200u64 };

    Ok(json!({
        "StatusCode": response_status,
        "Payload": mock_response,
    }))
}
