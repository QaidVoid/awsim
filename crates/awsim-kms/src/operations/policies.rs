use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::state::KmsState;
use crate::operations::keys::resolve_key_id;

const DEFAULT_POLICY_NAME: &str = "default";

// ---------------------------------------------------------------------------
// GetKeyPolicy
// ---------------------------------------------------------------------------

pub fn get_key_policy(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let policy_name = input["PolicyName"]
        .as_str()
        .unwrap_or(DEFAULT_POLICY_NAME);

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state.keys.get(&resolved_id).ok_or_else(|| error::not_found("Key"))?;

    let policy = key
        .policies
        .get(policy_name)
        .cloned()
        .unwrap_or_else(|| default_policy_document(&key.arn));

    Ok(json!({ "Policy": policy }))
}

// ---------------------------------------------------------------------------
// PutKeyPolicy
// ---------------------------------------------------------------------------

pub fn put_key_policy(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let policy_name = input["PolicyName"]
        .as_str()
        .unwrap_or(DEFAULT_POLICY_NAME);

    let policy = input["Policy"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Policy"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state.keys.get_mut(&resolved_id).ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    key.policies.insert(policy_name.to_string(), policy.to_string());

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListKeyPolicies
// ---------------------------------------------------------------------------

pub fn list_key_policies(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;

    if !state.keys.contains_key(&resolved_id) {
        return Err(error::not_found("Key"));
    }

    // AWS KMS only supports the "default" policy.
    Ok(json!({ "PolicyNames": [DEFAULT_POLICY_NAME], "Truncated": false }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_policy_document(key_arn: &str) -> String {
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"AWS":"*"}},"Action":"kms:*","Resource":"{key_arn}"}}]}}"#
    )
}
