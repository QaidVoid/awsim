use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error;
use crate::state::{KmsGrant, KmsState};
use crate::operations::keys::resolve_key_id;

// ---------------------------------------------------------------------------
// CreateGrant
// ---------------------------------------------------------------------------

pub fn create_grant(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let grantee_principal = input["GranteePrincipal"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("GranteePrincipal"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;

    // Key must exist
    if !state.keys.contains_key(&resolved_id) {
        return Err(error::not_found("Key"));
    }

    let operations: Vec<String> = input["Operations"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    let name = input["Name"].as_str().map(str::to_string);

    let grant_id = Uuid::new_v4().to_string().replace('-', "");
    let grant_token = Uuid::new_v4().to_string();

    let grant = KmsGrant {
        grant_id: grant_id.clone(),
        grant_token: grant_token.clone(),
        key_id: resolved_id,
        name,
        grantee_principal: grantee_principal.to_string(),
        operations,
    };

    state.grants.insert(grant_id.clone(), grant);

    Ok(json!({
        "GrantId": grant_id,
        "GrantToken": grant_token,
    }))
}

// ---------------------------------------------------------------------------
// ListGrants
// ---------------------------------------------------------------------------

pub fn list_grants(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;

    let grants: Vec<Value> = state
        .grants
        .iter()
        .filter(|e| e.value().key_id == resolved_id)
        .map(|e| grant_to_value(e.value()))
        .collect();

    Ok(json!({ "Grants": grants, "Truncated": false }))
}

// ---------------------------------------------------------------------------
// ListRetirableGrants
// ---------------------------------------------------------------------------

pub fn list_retirable_grants(
    _state: &KmsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Stub: retiring principal tracking is not implemented.
    Ok(json!({ "Grants": [], "Truncated": false }))
}

// ---------------------------------------------------------------------------
// RetireGrant
// ---------------------------------------------------------------------------

pub fn retire_grant(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Accept either GrantId or GrantToken.
    if let Some(grant_id) = input["GrantId"].as_str() {
        state.grants.remove(grant_id);
    } else if let Some(grant_token) = input["GrantToken"].as_str() {
        // Find grant by token and remove.
        let key = state
            .grants
            .iter()
            .find(|e| e.value().grant_token == grant_token)
            .map(|e| e.key().clone());
        if let Some(k) = key {
            state.grants.remove(&k);
        }
    }
    // Succeed silently even if not found (matches AWS behavior for RetireGrant).
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RevokeGrant
// ---------------------------------------------------------------------------

pub fn revoke_grant(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let grant_id = input["GrantId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("GrantId"))?;

    let _resolved_id = resolve_key_id(state, key_id_input)?;

    // Remove grant (succeed silently if absent — matches AWS behavior).
    state.grants.remove(grant_id);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn grant_to_value(grant: &KmsGrant) -> Value {
    json!({
        "GrantId": grant.grant_id,
        "KeyId": grant.key_id,
        "GranteePrincipal": grant.grantee_principal,
        "Operations": grant.operations,
        "Name": grant.name,
    })
}
