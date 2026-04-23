/// Miscellaneous IAM operations that are stubs or return empty data.
use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::IamState;

use super::require_str;

// ── ListServiceSpecificCredentials ────────────────────────────────────────────

/// ListServiceSpecificCredentials — Return an empty list.
/// Used for CodeCommit, Keyspaces, etc. credentials; none are tracked here.
pub fn list_service_specific_credentials(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    // Verify the user exists (AWS would 404 for unknown users).
    if !state.users.contains_key(user_name) {
        return Err(crate::error::no_such_entity("User", user_name));
    }

    Ok(json!({
        "ServiceSpecificCredentials": { "member": [] }
    }))
}

// ── ListSigningCertificates ───────────────────────────────────────────────────

/// ListSigningCertificates — Return an empty list.
pub fn list_signing_certificates(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    // user_name is optional
    if let Some(user_name) = input.get("UserName").and_then(|v| v.as_str()) {
        if !state.users.contains_key(user_name) {
            return Err(crate::error::no_such_entity("User", user_name));
        }
    }

    Ok(json!({
        "Certificates": { "member": [] },
        "IsTruncated": false
    }))
}

// ── Policy Simulator stubs ────────────────────────────────────────────────────

/// SimulateCustomPolicy — Stub that returns "allowed" for all actions.
pub fn simulate_custom_policy(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let action_names = input
        .get("ActionNames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = action_names
        .iter()
        .filter_map(|a| a.as_str())
        .map(|action| {
            json!({
                "EvalActionName": action,
                "EvalDecision": "allowed",
                "EvalResourceName": "*",
                "MatchedStatements": { "member": [] },
                "MissingContextValues": { "member": [] }
            })
        })
        .collect();

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

/// SimulatePrincipalPolicy — Stub that returns "allowed" for all actions.
pub fn simulate_principal_policy(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let action_names = input
        .get("ActionNames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = action_names
        .iter()
        .filter_map(|a| a.as_str())
        .map(|action| {
            json!({
                "EvalActionName": action,
                "EvalDecision": "allowed",
                "EvalResourceName": "*",
                "MatchedStatements": { "member": [] },
                "MissingContextValues": { "member": [] }
            })
        })
        .collect();

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

// ── GetContextKeys stubs ──────────────────────────────────────────────────────

/// GetContextKeysForCustomPolicy — Return empty context key list.
pub fn get_context_keys_for_custom_policy(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ContextKeyNames": { "member": [] }
    }))
}

/// GetContextKeysForPrincipalPolicy — Return empty context key list.
pub fn get_context_keys_for_principal_policy(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ContextKeyNames": { "member": [] }
    }))
}
