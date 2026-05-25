use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::error;
use crate::operations::keys::resolve_key_id;
use crate::state::{KeyRotationEvent, KmsState};
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
    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

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
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

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
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

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

    let key = state
        .keys
        .get(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;
    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }
    drop(key);

    // AWS rate-limits RotateKeyOnDemand to at most once every 24 hours per
    // customer-managed key (AWS-managed keys get 10/24h, but awsim treats
    // every key as customer-managed). Look at the most recent ON_DEMAND
    // rotation for this key and reject if it's still within the window.
    let now = now_epoch_f64();
    const ROTATE_WINDOW_SECS: f64 = 24.0 * 60.0 * 60.0;
    let too_soon = state
        .key_rotations
        .get(&resolved_id)
        .and_then(|events| {
            events
                .iter()
                .filter(|e| e.rotation_type == "ON_DEMAND")
                .map(|e| e.rotation_date)
                .fold(None::<f64>, |acc, t| Some(acc.map_or(t, |a| a.max(t))))
        })
        .is_some_and(|last| now - last < ROTATE_WINDOW_SECS);
    if too_soon {
        return Err(AwsError::bad_request(
            "LimitExceededException",
            format!("RotateKeyOnDemand is limited to once per 24 hours for key {resolved_id}.",),
        ));
    }

    let event = KeyRotationEvent {
        key_id: resolved_id.clone(),
        rotation_date: now,
        rotation_type: "ON_DEMAND".to_string(),
    };
    // Cap retained rotation history so a long-running awsim instance with
    // automated rotation doesn't grow the snapshot unbounded.
    const ROTATION_HISTORY_CAP: usize = 256;
    let mut events = state.key_rotations.entry(resolved_id.clone()).or_default();
    events.push(event);
    if events.len() > ROTATION_HISTORY_CAP {
        let excess = events.len() - ROTATION_HISTORY_CAP;
        events.drain(0..excess);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::keys::create_key;
    use crate::state::KmsState;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("kms", "us-east-1")
    }

    fn make_key(state: &KmsState) -> String {
        let resp = create_key(state, &json!({}), &ctx()).unwrap();
        resp["KeyMetadata"]["KeyId"].as_str().unwrap().to_string()
    }

    #[test]
    fn rotate_on_demand_succeeds_on_first_call() {
        let state = KmsState::default();
        let kid = make_key(&state);
        rotate_key_on_demand(&state, &json!({"KeyId": &kid}), &ctx()).unwrap();
    }

    #[test]
    fn rotate_on_demand_rejects_second_call_within_24h() {
        let state = KmsState::default();
        let kid = make_key(&state);
        rotate_key_on_demand(&state, &json!({"KeyId": &kid}), &ctx()).unwrap();
        let err = rotate_key_on_demand(&state, &json!({"KeyId": &kid}), &ctx()).unwrap_err();
        assert_eq!(err.code, "LimitExceededException");
    }

    #[test]
    fn rotate_on_demand_allowed_after_window_elapses() {
        let state = KmsState::default();
        let kid = make_key(&state);
        // Seed an old ON_DEMAND rotation outside the 24h window.
        state
            .key_rotations
            .entry(kid.clone())
            .or_default()
            .push(KeyRotationEvent {
                key_id: kid.clone(),
                rotation_date: now_epoch_f64() - (25.0 * 3600.0),
                rotation_type: "ON_DEMAND".to_string(),
            });
        rotate_key_on_demand(&state, &json!({"KeyId": &kid}), &ctx()).unwrap();
    }
}
