use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error;
use crate::operations::keys::resolve_key_id;
use crate::state::{KmsGrant, KmsState};

/// 5-minute lifetime documented for KMS grant tokens.
pub const GRANT_TOKEN_TTL_SECS: u64 = 300;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Validate every entry of `GrantTokens` (when present): each must
/// match an existing grant and be within the 5-minute TTL window.
/// AWS returns `InvalidGrantTokenException` for unknown or expired
/// tokens; mirror that so callers don't silently rely on stale tokens.
pub fn validate_grant_tokens(state: &KmsState, input: &Value) -> Result<(), AwsError> {
    let Some(tokens) = input.get("GrantTokens").and_then(Value::as_array) else {
        return Ok(());
    };
    let now = now_secs();
    for tok in tokens {
        let token = tok.as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidGrantTokenException",
                "GrantTokens entry must be a string.",
            )
        })?;
        let grant = state
            .grants
            .iter()
            .find(|e| e.value().grant_token == token)
            .map(|e| e.value().clone());
        match grant {
            None => {
                return Err(AwsError::bad_request(
                    "InvalidGrantTokenException",
                    format!("Grant token `{token}` is not valid."),
                ));
            }
            Some(g) if now.saturating_sub(g.token_created_at) > GRANT_TOKEN_TTL_SECS => {
                return Err(AwsError::bad_request(
                    "InvalidGrantTokenException",
                    format!(
                        "Grant token `{token}` has expired; tokens are valid for {GRANT_TOKEN_TTL_SECS} seconds after CreateGrant."
                    ),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

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

    // AWS limits grants to 50_000 per KMS key. Beyond that, CreateGrant
    // returns LimitExceededException.
    const GRANTS_PER_KEY_LIMIT: usize = 50_000;
    let live_grants_for_key = state
        .grants
        .iter()
        .filter(|e| e.value().key_id == resolved_id)
        .count();
    if live_grants_for_key >= GRANTS_PER_KEY_LIMIT {
        return Err(AwsError::bad_request(
            "LimitExceededException",
            format!("Key {resolved_id} already has the maximum {GRANTS_PER_KEY_LIMIT} grants."),
        ));
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
        token_created_at: now_secs(),
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

#[cfg(test)]
mod grant_token_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("kms", "us-east-1")
    }

    fn seed_grant(state: &KmsState, token: &str, created_at: u64) {
        state.grants.insert(
            "g1".into(),
            KmsGrant {
                grant_id: "g1".into(),
                grant_token: token.into(),
                key_id: "k1".into(),
                name: None,
                grantee_principal: "p".into(),
                operations: vec![],
                token_created_at: created_at,
            },
        );
    }

    #[test]
    fn fresh_token_passes_validation() {
        let state = KmsState::default();
        seed_grant(&state, "fresh", now_secs());
        validate_grant_tokens(&state, &json!({ "GrantTokens": ["fresh"] })).unwrap();
    }

    #[test]
    fn expired_token_returns_invalid_grant_token() {
        let state = KmsState::default();
        // 1 hour ago — past the 5-min window.
        seed_grant(&state, "stale", now_secs().saturating_sub(3600));
        let err = validate_grant_tokens(&state, &json!({ "GrantTokens": ["stale"] })).unwrap_err();
        assert_eq!(err.code, "InvalidGrantTokenException");
        assert!(err.message.contains("expired"));
    }

    #[test]
    fn unknown_token_returns_invalid_grant_token() {
        let state = KmsState::default();
        let err = validate_grant_tokens(&state, &json!({ "GrantTokens": ["bogus"] })).unwrap_err();
        assert_eq!(err.code, "InvalidGrantTokenException");
    }

    #[test]
    fn missing_grant_tokens_is_ok() {
        let state = KmsState::default();
        validate_grant_tokens(&state, &json!({})).unwrap();
    }

    #[test]
    fn create_grant_records_token_creation_time() {
        let state = KmsState::default();
        let key_id = "11111111-2222-3333-4444-555555555555";
        state.keys.insert(
            key_id.to_string(),
            crate::state::KmsKey {
                key_id: key_id.to_string(),
                arn: format!("arn:aws:kms:us-east-1:000000000000:key/{key_id}"),
                description: String::new(),
                key_state: "Enabled".to_string(),
                key_spec: "SYMMETRIC_DEFAULT".to_string(),
                key_usage: "ENCRYPT_DECRYPT".to_string(),
                creation_date: 0.0,
                secret: vec![0u8; 32],
                deletion_date: None,
                rotation_enabled: false,
                policies: Default::default(),
                tags: Default::default(),
                key_material_imported: false,
                origin: "AWS_KMS".to_string(),
            },
        );
        let resp = create_grant(
            &state,
            &json!({
                "KeyId": key_id,
                "GranteePrincipal": "arn:aws:iam::000000000000:user/x",
            }),
            &ctx(),
        )
        .unwrap();
        let token = resp["GrantToken"].as_str().unwrap();
        validate_grant_tokens(&state, &json!({ "GrantTokens": [token] })).unwrap();
    }
}
