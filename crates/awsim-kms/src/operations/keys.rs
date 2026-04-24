use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error;
use crate::state::{KmsKey, KmsState};
use crate::util::{now_epoch_f64, random_secret};

// ---------------------------------------------------------------------------
// CreateKey
// ---------------------------------------------------------------------------

pub fn create_key(
    state: &KmsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_spec = input["KeySpec"]
        .as_str()
        .unwrap_or("SYMMETRIC_DEFAULT")
        .to_string();
    let key_usage = input["KeyUsage"]
        .as_str()
        .unwrap_or("ENCRYPT_DECRYPT")
        .to_string();
    let description = input["Description"].as_str().unwrap_or("").to_string();

    // Validate key_spec / key_usage compatibility
    if key_spec == "SYMMETRIC_DEFAULT" && key_usage != "ENCRYPT_DECRYPT" {
        return Err(error::invalid_parameter(
            "SYMMETRIC_DEFAULT keys must have ENCRYPT_DECRYPT key usage",
        ));
    }

    let key_id = Uuid::new_v4().to_string();
    let arn = format!(
        "arn:aws:kms:{}:{}:key/{}",
        ctx.region, ctx.account_id, key_id
    );
    let secret = random_secret(32);
    let creation_date = now_epoch_f64();

    let key = KmsKey {
        key_id: key_id.clone(),
        arn: arn.clone(),
        description: description.clone(),
        key_state: "Enabled".to_string(),
        key_spec: key_spec.clone(),
        key_usage: key_usage.clone(),
        creation_date,
        secret,
        deletion_date: None,
        rotation_enabled: false,
        policies: HashMap::new(),
        tags: HashMap::new(),
        key_material_imported: false,
        origin: "AWS_KMS".to_string(),
    };

    state.keys.insert(key_id.clone(), key);

    Ok(json!({
        "KeyMetadata": {
            "KeyId": key_id,
            "Arn": arn,
            "Description": description,
            "KeyState": "Enabled",
            "KeySpec": key_spec,
            "KeyUsage": key_usage,
            "CreationDate": creation_date,
            "Enabled": true,
            "KeyManager": "CUSTOMER",
            "Origin": "AWS_KMS",
            "MultiRegion": false,
        }
    }))
}

// ---------------------------------------------------------------------------
// DescribeKey
// ---------------------------------------------------------------------------

pub fn describe_key(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let key = resolve_key(state, key_id_input)?;

    Ok(json!({
        "KeyMetadata": key_metadata(&key)
    }))
}

// ---------------------------------------------------------------------------
// ListKeys
// ---------------------------------------------------------------------------

pub fn list_keys(
    state: &KmsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let keys: Vec<Value> = state
        .keys
        .iter()
        .map(|entry| {
            json!({
                "KeyId": entry.key_id,
                "KeyArn": entry.arn,
            })
        })
        .collect();

    Ok(json!({ "Keys": keys, "Truncated": false }))
}

// ---------------------------------------------------------------------------
// EnableKey
// ---------------------------------------------------------------------------

pub fn enable_key(
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

    key.key_state = "Enabled".to_string();
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DisableKey
// ---------------------------------------------------------------------------

pub fn disable_key(
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

    key.key_state = "Disabled".to_string();
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ScheduleKeyDeletion
// ---------------------------------------------------------------------------

pub fn schedule_key_deletion(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let pending_window_in_days = input["PendingWindowInDays"].as_u64().unwrap_or(30);

    if !(7..=30).contains(&pending_window_in_days) {
        return Err(error::invalid_parameter(
            "PendingWindowInDays must be between 7 and 30",
        ));
    }

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    // Compute deletion date: now + pending_window_in_days seconds.
    use std::time::{SystemTime, UNIX_EPOCH};
    let deletion_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        + (pending_window_in_days * 86400) as f64;

    key.key_state = "PendingDeletion".to_string();
    key.deletion_date = Some(deletion_epoch);

    let key_id = key.key_id.clone();
    drop(key);

    Ok(json!({
        "KeyId": key_id,
        "DeletionDate": deletion_epoch,
        "PendingWindowInDays": pending_window_in_days,
    }))
}

// ---------------------------------------------------------------------------
// CancelKeyDeletion
// ---------------------------------------------------------------------------

pub fn cancel_key_deletion(
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

    if key.key_state != "PendingDeletion" {
        return Err(AwsError::bad_request(
            "KMSInvalidStateException",
            format!("Key {resolved_id} is not pending deletion"),
        ));
    }

    key.key_state = "Disabled".to_string();
    key.deletion_date = None;
    let key_id = key.key_id.clone();
    drop(key);

    Ok(json!({ "KeyId": key_id }))
}

// ---------------------------------------------------------------------------
// UpdateKeyDescription
// ---------------------------------------------------------------------------

pub fn update_key_description(
    state: &KmsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let key_id_input = input["KeyId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("KeyId"))?;

    let description = input["Description"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Description"))?;

    let resolved_id = resolve_key_id(state, key_id_input)?;
    let mut key = state
        .keys
        .get_mut(&resolved_id)
        .ok_or_else(|| error::not_found("Key"))?;

    if key.key_state == "PendingDeletion" {
        return Err(error::key_pending_deletion(&resolved_id));
    }

    key.description = description.to_string();
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve key_id_input which may be a KeyId, ARN, or alias name to the actual KeyId string.
pub fn resolve_key_id(state: &KmsState, input: &str) -> Result<String, AwsError> {
    // ARN format: arn:aws:kms:...:key/<uuid>
    if input.starts_with("arn:aws:kms:") {
        let key_id = input
            .rsplit('/')
            .next()
            .ok_or_else(|| error::invalid_key_id(input))?;
        if state.keys.contains_key(key_id) {
            return Ok(key_id.to_string());
        }
        return Err(error::not_found("Key"));
    }

    // Alias ARN: arn:aws:kms:...:alias/<name>
    if input.starts_with("arn:aws:kms:") && input.contains(":alias/") {
        let alias_part = input
            .split(":alias/")
            .nth(1)
            .map(|s| format!("alias/{s}"))
            .ok_or_else(|| error::invalid_key_id(input))?;
        if let Some(key_id) = state.aliases.get(&alias_part) {
            return Ok(key_id.clone());
        }
        return Err(error::not_found("Alias"));
    }

    // Alias name (starts with "alias/")
    if input.starts_with("alias/") {
        if let Some(key_id) = state.aliases.get(input) {
            return Ok(key_id.clone());
        }
        return Err(error::not_found("Alias"));
    }

    // Plain key ID (UUID)
    if state.keys.contains_key(input) {
        return Ok(input.to_string());
    }

    Err(error::not_found("Key"))
}

/// Resolve key, returning the key clone.
pub fn resolve_key(state: &KmsState, input: &str) -> Result<KmsKey, AwsError> {
    let key_id = resolve_key_id(state, input)?;
    state
        .keys
        .get(&key_id)
        .map(|r| r.clone())
        .ok_or_else(|| error::not_found("Key"))
}

pub fn key_metadata(key: &KmsKey) -> Value {
    let enabled = key.key_state == "Enabled";
    let mut meta = json!({
        "KeyId": key.key_id,
        "Arn": key.arn,
        "Description": key.description,
        "KeyState": key.key_state,
        "KeySpec": key.key_spec,
        "KeyUsage": key.key_usage,
        "CreationDate": key.creation_date,
        "Enabled": enabled,
        "KeyManager": "CUSTOMER",
        "Origin": key.origin,
        "MultiRegion": false,
    });

    if let Some(ref dd) = key.deletion_date {
        meta["DeletionDate"] = json!(dd);
    }
    meta
}

#[allow(dead_code)]
fn format_iso8601(secs: u64) -> String {
    // reuse the util function logic inline for now
    crate::util::secs_to_iso8601(secs)
}
