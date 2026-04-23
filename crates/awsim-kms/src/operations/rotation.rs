use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::state::{KeyRotationEvent, KmsState};
use crate::operations::keys::resolve_key_id;
use crate::util::now_epoch_f64;

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

pub fn rotate_key_on_demand(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    let resolved_id = resolve_key_id(state, key_id_input)?;

    let key = state.keys.get(&resolved_id).ok_or_else(|| error::not_found("Key"))?;
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }
    drop(key);

    let event = KeyRotationEvent {
        key_id: resolved_id.clone(),
        rotation_date: now_epoch_f64(),
        rotation_type: "ON_DEMAND".to_string(),
    };
    state
        .key_rotations
        .entry(resolved_id.clone())
        .or_default()
        .push(event);

    Ok(json!({ "KeyId": resolved_id }))
}

pub fn list_key_rotations(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;
    let resolved_id = resolve_key_id(state, key_id_input)?;

    let rotations: Vec<Value> = state
        .key_rotations
        .get(&resolved_id)
        .map(|entries| {
            entries
                .iter()
                .map(|e| {
                    json!({
                        "KeyId": e.key_id,
                        "RotationDate": e.rotation_date,
                        "RotationType": e.rotation_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "Rotations": rotations,
        "Truncated": false,
    }))
}
