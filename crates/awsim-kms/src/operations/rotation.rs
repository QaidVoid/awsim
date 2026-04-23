use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::state::KmsState;
use crate::operations::keys::resolve_key_id;

// ---------------------------------------------------------------------------
// GetKeyRotationStatus
// ---------------------------------------------------------------------------

pub fn get_key_rotation_status(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let key = state.keys.get(&resolved_id).ok_or_else(|| error::not_found("Key"))?;

    Ok(json!({ "KeyRotationEnabled": key.rotation_enabled }))
}

// ---------------------------------------------------------------------------
// EnableKeyRotation
// ---------------------------------------------------------------------------

pub fn enable_key_rotation(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state.keys.get_mut(&resolved_id).ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    key.rotation_enabled = true;
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DisableKeyRotation
// ---------------------------------------------------------------------------

pub fn disable_key_rotation(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state.keys.get_mut(&resolved_id).ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    key.rotation_enabled = false;
    Ok(json!({}))
}
