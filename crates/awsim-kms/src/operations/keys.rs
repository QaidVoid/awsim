use std::collections::HashMap;

use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext, arn};
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
    let origin = input["Origin"].as_str().unwrap_or("AWS_KMS").to_string();

    // Validate key_spec / key_usage compatibility
    if key_spec == "SYMMETRIC_DEFAULT" && key_usage != "ENCRYPT_DECRYPT" {
        return Err(error::invalid_parameter(
            "SYMMETRIC_DEFAULT keys must have ENCRYPT_DECRYPT key usage",
        ));
    }

    // EXTERNAL-origin keys start in PendingImport with no usable key
    // material; the customer must run GetParametersForImport followed by
    // ImportKeyMaterial before the key transitions to Enabled. AWS_KMS
    // (default) and AWS_CLOUDHSM origins are immediately Enabled.
    let initial_state = match origin.as_str() {
        "EXTERNAL" => "PendingImport",
        _ => "Enabled",
    };
    let initial_enabled = initial_state == "Enabled";

    let key_id = Uuid::new_v4().to_string();
    let arn = arn::build(ctx, "kms", format!("key/{key_id}"));
    let secret = random_secret(32);
    let creation_date = now_epoch_f64();

    let key = KmsKey {
        key_id: key_id.clone(),
        arn: arn.clone(),
        description: description.clone(),
        key_state: initial_state.to_string(),
        key_spec: key_spec.clone(),
        key_usage: key_usage.clone(),
        creation_date,
        secret,
        deletion_date: None,
        rotation_enabled: false,
        policies: HashMap::new(),
        tags: HashMap::new(),
        key_material_imported: false,
        origin: origin.clone(),
    };

    state.keys.insert(key_id.clone(), key);

    Ok(json!({
        "KeyMetadata": {
            "KeyId": key_id,
            "Arn": arn,
            "Description": description,
            "KeyState": initial_state,
            "KeySpec": key_spec,
            "KeyUsage": key_usage,
            "CreationDate": creation_date,
            "Enabled": initial_enabled,
            "KeyManager": "CUSTOMER",
            "Origin": origin,
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
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let limit = cap_max_results(input["Limit"].as_i64(), 100, 1000);
    let mut items: Vec<(String, Value)> = state
        .keys
        .iter()
        .map(|entry| {
            (
                entry.key_id.clone(),
                json!({ "KeyId": entry.key_id, "KeyArn": entry.arn }),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let page = paginate(items, limit, input["Marker"].as_str(), |(id, _)| id.clone())?;
    let keys: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    let mut resp = json!({ "Keys": keys, "Truncated": page.next_token.is_some() });
    if let Some(marker) = page.next_token {
        resp["NextMarker"] = json!(marker);
    }
    Ok(resp)
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
        return Err(error::kms_invalid_state(format!(
            "Key {resolved_id} is not pending deletion"
        )));
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

/// Resolve a key identifier (KeyId, key ARN, alias name, or alias ARN)
/// to the canonical KeyId string.
///
/// The order matters: alias ARNs are checked before key ARNs because
/// both share the `arn:{partition}:kms:` prefix; a generic `:kms:…/X`
/// match would otherwise treat the alias name as a raw key ID and fail
/// the key-table lookup before the alias path is considered.
pub fn resolve_key_id(state: &KmsState, input: &str) -> Result<String, AwsError> {
    // Alias ARN: arn:{partition}:kms:{region}:{account}:alias/{name}
    if input.starts_with("arn:") && input.contains(":kms:") && input.contains(":alias/") {
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

    // Key ARN: arn:{partition}:kms:{region}:{account}:key/{uuid}
    if input.starts_with("arn:") && input.contains(":kms:") {
        let key_id = input
            .rsplit('/')
            .next()
            .ok_or_else(|| error::invalid_key_id(input))?;
        if state.keys.contains_key(key_id) {
            return Ok(key_id.to_string());
        }
        return Err(error::not_found("Key"));
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
