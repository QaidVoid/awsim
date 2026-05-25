//! Reserved + provisioned concurrency operations.
//!
//! Real Lambda gates invocations against `reserved_concurrent_executions`
//! and tracks an asynchronous IN_PROGRESS -> READY transition for
//! provisioned concurrency. The emulator keeps the bookkeeping (so SDK
//! clients see the values they wrote round-trip) and flips the state
//! immediately because there's no real warm-pool latency to model.

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error::{invalid_parameter, missing_parameter, resource_not_found};
use crate::state::{LambdaState, ProvisionedConcurrencyConfig};
use crate::util::{now_iso8601, require_str, validate_qualifier};

const ACCOUNT_UNRESERVED_CAP: u32 = 1000;

// ---------------------------------------------------------------------------
// Reserved concurrency
// ---------------------------------------------------------------------------

/// PUT /2017-10-31/functions/{FunctionName}/concurrency
pub fn put_function_concurrency(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let reserved = input
        .get("ReservedConcurrentExecutions")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_parameter("ReservedConcurrentExecutions"))?;
    if reserved > ACCOUNT_UNRESERVED_CAP as u64 {
        return Err(invalid_parameter(format!(
            "ReservedConcurrentExecutions {reserved} exceeds the account unreserved limit of {ACCOUNT_UNRESERVED_CAP}"
        )));
    }
    let mut function = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    function.reserved_concurrent_executions = Some(reserved as u32);
    Ok(json!({ "ReservedConcurrentExecutions": reserved }))
}

/// GET /2019-09-30/functions/{FunctionName}/concurrency
pub fn get_function_concurrency(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let function = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    let mut resp = serde_json::Map::new();
    if let Some(n) = function.reserved_concurrent_executions {
        resp.insert("ReservedConcurrentExecutions".into(), json!(n));
    }
    Ok(Value::Object(resp))
}

/// DELETE /2017-10-31/functions/{FunctionName}/concurrency
pub fn delete_function_concurrency(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let mut function = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    function.reserved_concurrent_executions = None;
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Provisioned concurrency
// ---------------------------------------------------------------------------

/// PUT /2019-09-30/functions/{FunctionName}/provisioned-concurrency?Qualifier=...
pub fn put_provisioned_concurrency_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let qualifier = require_str(input, "Qualifier")?;
    validate_qualifier(qualifier)?;
    let requested = input
        .get("ProvisionedConcurrentExecutions")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_parameter("ProvisionedConcurrentExecutions"))?;
    if requested == 0 {
        return Err(invalid_parameter(
            "ProvisionedConcurrentExecutions must be at least 1",
        ));
    }
    let mut function = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    // Provisioned concurrency targets a published version or alias — never
    // $LATEST. Mirror the real validation so misconfigured callers see the
    // same error path locally as in production.
    if qualifier == "$LATEST" {
        return Err(invalid_parameter(
            "Provisioned concurrency cannot target $LATEST. Use a published version or alias.",
        ));
    }

    let now = now_iso8601();
    function.provisioned_concurrency.insert(
        qualifier.to_string(),
        ProvisionedConcurrencyConfig {
            qualifier: qualifier.to_string(),
            requested_provisioned_concurrent_executions: requested as u32,
            allocated_provisioned_concurrent_executions: requested as u32,
            available_provisioned_concurrent_executions: requested as u32,
            status: "READY".to_string(),
            status_reason: None,
            last_modified: now.clone(),
        },
    );

    Ok(provisioned_to_value(
        function.provisioned_concurrency.get(qualifier).unwrap(),
    ))
}

/// GET /2019-09-30/functions/{FunctionName}/provisioned-concurrency?Qualifier=...
pub fn get_provisioned_concurrency_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let qualifier = require_str(input, "Qualifier")?;
    validate_qualifier(qualifier)?;
    let function = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    let cfg = function
        .provisioned_concurrency
        .get(qualifier)
        .ok_or_else(|| {
            AwsError::not_found(
                "ProvisionedConcurrencyConfigNotFoundException",
                format!("No provisioned concurrency config for {name}:{qualifier}"),
            )
        })?;
    Ok(provisioned_to_value(cfg))
}

/// DELETE /2019-09-30/functions/{FunctionName}/provisioned-concurrency?Qualifier=...
pub fn delete_provisioned_concurrency_config(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let qualifier = require_str(input, "Qualifier")?;
    validate_qualifier(qualifier)?;
    let mut function = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    function.provisioned_concurrency.remove(qualifier);
    Ok(json!({}))
}

/// GET /2019-09-30/functions/{FunctionName}/provisioned-concurrency
pub fn list_provisioned_concurrency_configs(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let function = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;
    let configs: Vec<Value> = function
        .provisioned_concurrency
        .values()
        .map(provisioned_to_value)
        .collect();
    Ok(json!({ "ProvisionedConcurrencyConfigs": configs }))
}

fn provisioned_to_value(cfg: &ProvisionedConcurrencyConfig) -> Value {
    let mut v = json!({
        "RequestedProvisionedConcurrentExecutions": cfg.requested_provisioned_concurrent_executions,
        "AllocatedProvisionedConcurrentExecutions": cfg.allocated_provisioned_concurrent_executions,
        "AvailableProvisionedConcurrentExecutions": cfg.available_provisioned_concurrent_executions,
        "Status": cfg.status,
        "LastModified": cfg.last_modified,
    });
    if let Some(reason) = &cfg.status_reason {
        v["StatusReason"] = Value::String(reason.clone());
    }
    v
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::state::LambdaFunction;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn state_with_function(name: &str) -> LambdaState {
        let state = LambdaState::default();
        state.functions.insert(
            name.to_string(),
            LambdaFunction {
                name: name.to_string(),
                arn: format!("arn:aws:lambda:us-east-1:000000000000:function:{name}"),
                runtime: Some("nodejs20.x".into()),
                role: "arn:aws:iam::000000000000:role/test".into(),
                handler: Some("index.handler".into()),
                description: String::new(),
                timeout: 3,
                memory_size: 128,
                code_sha256: String::new(),
                code_size: 0,
                code: None,
                environment: HashMap::new(),
                version: "$LATEST".into(),
                versions: vec![],
                aliases: HashMap::new(),
                last_modified: "now".into(),
                state: "Active".into(),
                invocations: vec![],
                policy_statements: HashMap::new(),
                tags: HashMap::new(),
                reserved_concurrent_executions: None,
                provisioned_concurrency: HashMap::new(),
                architectures: vec!["x86_64".into()],
                ephemeral_storage_size: 512,
                package_type: "Zip".into(),
                layers: vec![],
                vpc_config: None,
                dead_letter_config: None,
                tracing_config: None,
                kms_key_arn: None,
                file_system_configs: None,
                logging_config: None,
                snap_start: None,
                image_config: None,
            },
        );
        state
    }

    #[test]
    fn put_get_delete_reserved_concurrency_round_trip() {
        let state = state_with_function("f");
        // Initially absent — Get returns the empty shape (no field).
        let got =
            get_function_concurrency(&state, &serde_json::json!({ "FunctionName": "f" }), &ctx())
                .unwrap();
        assert!(got.get("ReservedConcurrentExecutions").is_none());

        put_function_concurrency(
            &state,
            &serde_json::json!({
                "FunctionName": "f",
                "ReservedConcurrentExecutions": 25,
            }),
            &ctx(),
        )
        .unwrap();
        let got =
            get_function_concurrency(&state, &serde_json::json!({ "FunctionName": "f" }), &ctx())
                .unwrap();
        assert_eq!(got["ReservedConcurrentExecutions"], serde_json::json!(25));

        delete_function_concurrency(&state, &serde_json::json!({ "FunctionName": "f" }), &ctx())
            .unwrap();
        let got =
            get_function_concurrency(&state, &serde_json::json!({ "FunctionName": "f" }), &ctx())
                .unwrap();
        assert!(got.get("ReservedConcurrentExecutions").is_none());
    }

    #[test]
    fn put_function_concurrency_rejects_above_account_cap() {
        let state = state_with_function("f");
        let err = put_function_concurrency(
            &state,
            &serde_json::json!({
                "FunctionName": "f",
                "ReservedConcurrentExecutions": 5000,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn provisioned_concurrency_rejects_latest_qualifier() {
        let state = state_with_function("f");
        let err = put_provisioned_concurrency_config(
            &state,
            &serde_json::json!({
                "FunctionName": "f",
                "Qualifier": "$LATEST",
                "ProvisionedConcurrentExecutions": 1,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn provisioned_concurrency_round_trip_and_list() {
        let state = state_with_function("f");
        put_provisioned_concurrency_config(
            &state,
            &serde_json::json!({
                "FunctionName": "f",
                "Qualifier": "live",
                "ProvisionedConcurrentExecutions": 4,
            }),
            &ctx(),
        )
        .unwrap();
        let got = get_provisioned_concurrency_config(
            &state,
            &serde_json::json!({ "FunctionName": "f", "Qualifier": "live" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(got["Status"], serde_json::json!("READY"));
        assert_eq!(
            got["AvailableProvisionedConcurrentExecutions"],
            serde_json::json!(4)
        );

        let list = list_provisioned_concurrency_configs(
            &state,
            &serde_json::json!({ "FunctionName": "f" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            list["ProvisionedConcurrencyConfigs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        delete_provisioned_concurrency_config(
            &state,
            &serde_json::json!({ "FunctionName": "f", "Qualifier": "live" }),
            &ctx(),
        )
        .unwrap();
        let err = get_provisioned_concurrency_config(
            &state,
            &serde_json::json!({ "FunctionName": "f", "Qualifier": "live" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ProvisionedConcurrencyConfigNotFoundException");
    }
}
