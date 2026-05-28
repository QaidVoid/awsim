use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{EfsState, FileSystem};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_fs_id() -> String {
    format!("fs-{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

fn fs_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:elasticfilesystem:{}:{}:file-system/{}",
        ctx.region, ctx.account_id, id
    )
}

fn fs_to_value(fs: &FileSystem) -> Value {
    // Refresh the SizeInBytes.Timestamp on every read so callers can
    // see the metric as "current". Real EFS only refreshes on its own
    // metric-collection cadence, but the emulator has no tick driver
    // yet.
    let now = now_secs();
    json!({
        "FileSystemId": fs.file_system_id,
        "FileSystemArn": fs.file_system_arn,
        "CreationToken": fs.creation_token,
        "CreationTime": fs.creation_time,
        "LifeCycleState": fs.life_cycle_state,
        "NumberOfMountTargets": fs.number_of_mount_targets,
        "SizeInBytes": {
            "Value": fs.size_in_bytes_value,
            "ValueInIA": 0,
            "ValueInStandard": fs.size_in_bytes_value,
            "ValueInArchive": 0,
            "Timestamp": now,
        },
        "PerformanceMode": fs.performance_mode,
        "ThroughputMode": fs.throughput_mode,
        "ProvisionedThroughputInMibps": fs.provisioned_throughput_in_mibps,
        "Encrypted": fs.encrypted,
        "KmsKeyId": fs.kms_key_id,
        "Name": fs.name,
        "Tags": tags_to_array(&fs.tags),
        "OwnerId": "000000000000",
        "FileSystemProtection": {
            "ReplicationOverwriteProtection": fs.file_system_protection_replication_overwrite_protection,
        },
        "AvailabilityZoneName": fs.availability_zone_name,
        "AvailabilityZoneId": fs.availability_zone_id,
    })
}

fn tags_to_array(tags: &HashMap<String, String>) -> Value {
    Value::Array(
        tags.iter()
            .map(|(k, v)| json!({ "Key": k, "Value": v }))
            .collect(),
    )
}

fn tags_from_input(input: &Value) -> HashMap<String, String> {
    input
        .get("Tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let k = t.get("Key")?.as_str()?.to_string();
                    let v = t.get("Value")?.as_str()?.to_string();
                    Some((k, v))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Compare the immutable creation-time args from a fresh `CreateFileSystem`
/// call against an existing file system. AWS treats two calls with the same
/// `CreationToken` but differing values for these fields as a conflict.
fn creation_args_match(fs: &FileSystem, input: &Value) -> bool {
    let perf = input
        .get("PerformanceMode")
        .and_then(|v| v.as_str())
        .unwrap_or("generalPurpose");
    if fs.performance_mode != perf {
        return false;
    }
    let encrypted = input
        .get("Encrypted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if fs.encrypted != encrypted {
        return false;
    }
    let kms = input.get("KmsKeyId").and_then(|v| v.as_str());
    if let Some(k) = kms
        && fs.kms_key_id.as_deref() != Some(k)
    {
        return false;
    }
    let tmode = input
        .get("ThroughputMode")
        .and_then(|v| v.as_str())
        .unwrap_or("bursting");
    if fs.throughput_mode != tmode {
        return false;
    }
    let mibps = input
        .get("ProvisionedThroughputInMibps")
        .and_then(|v| v.as_f64());
    if fs.provisioned_throughput_in_mibps != mibps {
        return false;
    }
    let az_name = input.get("AvailabilityZoneName").and_then(|v| v.as_str());
    if fs.availability_zone_name.as_deref() != az_name {
        return false;
    }
    let az_id = input.get("AvailabilityZoneId").and_then(|v| v.as_str());
    if fs.availability_zone_id.as_deref() != az_id {
        return false;
    }
    true
}

pub fn create_file_system(
    state: &EfsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input
        .get("CreationToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "CreationToken is required"))?
        .to_string();

    // Idempotency: a replay with the same CreationToken and identical
    // immutable args returns the cached FS; mismatched args raise
    // FileSystemAlreadyExists. AWS gates parity on the creation-time
    // PerformanceMode/Encrypted/KmsKeyId/ThroughputMode/
    // ProvisionedThroughputInMibps/AvailabilityZoneName/Id tuple.
    if let Some(existing) = state
        .file_systems
        .iter()
        .find(|e| e.value().creation_token == token)
    {
        let fs = existing.value();
        if creation_args_match(fs, input) {
            return Ok(fs_to_value(fs));
        }
        return Err(AwsError::conflict(
            "FileSystemAlreadyExists",
            format!(
                "File system {} already exists with CreationToken `{}`.",
                fs.file_system_id, token
            ),
        ));
    }

    let id = new_fs_id();
    let tags = tags_from_input(input);
    let name = tags
        .get("Name")
        .cloned()
        .or_else(|| input.get("Name").and_then(|v| v.as_str()).map(String::from));

    let throughput_mode = input
        .get("ThroughputMode")
        .and_then(|v| v.as_str())
        .unwrap_or("bursting")
        .to_string();
    let provisioned_throughput_in_mibps = input
        .get("ProvisionedThroughputInMibps")
        .and_then(|v| v.as_f64());
    // AWS allows ProvisionedThroughputInMibps in 1..=1024; required when
    // ThroughputMode=provisioned and rejected otherwise.
    if throughput_mode == "provisioned" {
        let mibps = provisioned_throughput_in_mibps.ok_or_else(|| {
            AwsError::bad_request(
                "BadRequest",
                "ProvisionedThroughputInMibps is required when ThroughputMode=provisioned.",
            )
        })?;
        if !(1.0..=1024.0).contains(&mibps) {
            return Err(AwsError::bad_request(
                "BadRequest",
                format!("ProvisionedThroughputInMibps `{mibps}` must be in 1..=1024."),
            ));
        }
    } else if provisioned_throughput_in_mibps.is_some() {
        return Err(AwsError::bad_request(
            "BadRequest",
            "ProvisionedThroughputInMibps is only allowed when ThroughputMode=provisioned.",
        ));
    }

    let encrypted = input
        .get("Encrypted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let supplied_kms_key = input
        .get("KmsKeyId")
        .and_then(|v| v.as_str())
        .map(String::from);
    // AWS lets EFS default to the AWS-managed CMK alias when
    // Encrypted=true and no KmsKeyId is provided. Encrypted=false
    // rejects KmsKeyId outright.
    let kms_key_id = match (encrypted, supplied_kms_key) {
        (false, Some(_)) => {
            return Err(AwsError::bad_request(
                "BadRequest",
                "KmsKeyId is only allowed when Encrypted=true.",
            ));
        }
        (true, Some(arn)) => Some(arn),
        (true, None) => Some(format!(
            "arn:aws:kms:{}:{}:alias/aws/elasticfilesystem",
            ctx.region, ctx.account_id
        )),
        (false, None) => None,
    };
    let fs = FileSystem {
        file_system_id: id.clone(),
        file_system_arn: fs_arn(ctx, &id),
        creation_token: token,
        creation_time: now_secs(),
        life_cycle_state: "available".to_string(),
        number_of_mount_targets: 0,
        size_in_bytes_value: 0,
        performance_mode: input
            .get("PerformanceMode")
            .and_then(|v| v.as_str())
            .unwrap_or("generalPurpose")
            .to_string(),
        throughput_mode,
        provisioned_throughput_in_mibps,
        encrypted,
        kms_key_id,
        name,
        tags,
        lifecycle_policies: vec![],
        backup_policy_status: "DISABLED".to_string(),
        file_system_protection_replication_overwrite_protection: "ENABLED".to_string(),
        availability_zone_name: input
            .get("AvailabilityZoneName")
            .and_then(|v| v.as_str())
            .map(String::from),
        availability_zone_id: input
            .get("AvailabilityZoneId")
            .and_then(|v| v.as_str())
            .map(String::from),
        file_system_policy: None,
        throughput_last_modified: None,
    };
    let result = fs_to_value(&fs);
    state.file_systems.insert(id, fs);
    Ok(result)
}

pub fn describe_file_systems(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_filter = input.get("FileSystemId").and_then(|v| v.as_str());
    let token_filter = input.get("CreationToken").and_then(|v| v.as_str());
    let max_items = awsim_core::clamp_max_results_strict(
        input.get("MaxItems").and_then(Value::as_i64),
        100,
        1000,
    )?;
    let marker = input.get("Marker").and_then(Value::as_str);
    let mut entries: Vec<(String, Value)> = state
        .file_systems
        .iter()
        .filter(|e| {
            if let Some(id) = id_filter
                && e.value().file_system_id != id
            {
                return false;
            }
            if let Some(t) = token_filter
                && e.value().creation_token != t
            {
                return false;
            }
            true
        })
        .map(|e| (e.value().file_system_id.clone(), fs_to_value(e.value())))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let page = awsim_core::paginate(entries, max_items, marker, |(k, _)| k.clone())?;
    let items: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();
    let mut body = json!({ "FileSystems": items });
    if let Some(token) = page.next_token {
        body["NextMarker"] = json!(token);
    }
    Ok(body)
}

pub fn delete_file_system(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    if state
        .mount_targets
        .iter()
        .any(|e| e.value().file_system_id == id)
    {
        return Err(AwsError::bad_request(
            "FileSystemInUse",
            "Delete mount targets before deleting the file system",
        ));
    }
    if state
        .access_points
        .iter()
        .any(|e| e.value().file_system_id == id)
    {
        return Err(AwsError::bad_request(
            "FileSystemInUse",
            "Delete access points before deleting the file system",
        ));
    }
    state.file_systems.remove(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    Ok(json!({}))
}

pub fn update_file_system(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    let new_mode = input
        .get("ThroughputMode")
        .and_then(|v| v.as_str())
        .map(String::from);
    let new_mibps = input
        .get("ProvisionedThroughputInMibps")
        .and_then(|v| v.as_f64());
    let effective_mode = new_mode
        .clone()
        .unwrap_or_else(|| fs.throughput_mode.clone());
    if effective_mode == "provisioned" {
        let mibps = new_mibps
            .or(fs.provisioned_throughput_in_mibps)
            .ok_or_else(|| {
                AwsError::bad_request(
                    "BadRequest",
                    "ProvisionedThroughputInMibps is required when ThroughputMode=provisioned.",
                )
            })?;
        if !(1.0..=1024.0).contains(&mibps) {
            return Err(AwsError::bad_request(
                "BadRequest",
                format!("ProvisionedThroughputInMibps `{mibps}` must be in 1..=1024."),
            ));
        }
    } else if new_mibps.is_some() {
        return Err(AwsError::bad_request(
            "BadRequest",
            "ProvisionedThroughputInMibps is only allowed when ThroughputMode=provisioned.",
        ));
    }
    let mode_changed = new_mode.as_ref().is_some_and(|m| m != &fs.throughput_mode);
    let mibps_changed = new_mibps.is_some_and(|m| Some(m) != fs.provisioned_throughput_in_mibps);
    if mode_changed || mibps_changed {
        let cooldown = throughput_cooldown_secs();
        if let Some(last) = fs.throughput_last_modified {
            let elapsed = now_secs() - last;
            if elapsed < cooldown {
                let remaining = (cooldown - elapsed).max(0.0) as u64;
                return Err(AwsError::bad_request(
                    "ThroughputLimitExceeded",
                    format!(
                        "Cannot change throughput within 24 hours of the last change; \
                         retry in {remaining}s.",
                    ),
                ));
            }
        }
        fs.throughput_last_modified = Some(now_secs());
    }
    if let Some(mode) = new_mode {
        fs.throughput_mode = mode;
    }
    if let Some(p) = new_mibps {
        fs.provisioned_throughput_in_mibps = Some(p);
    }
    Ok(fs_to_value(&fs))
}

/// Throughput-change cooldown in seconds. Defaults to 24h to match
/// AWS; tests and demos can short-circuit to ~0 by setting
/// `AWSIM_LIFECYCLE_FAST=1`.
fn throughput_cooldown_secs() -> f64 {
    if std::env::var("AWSIM_LIFECYCLE_FAST")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        0.0
    } else {
        24.0 * 3600.0
    }
}

pub fn put_lifecycle_configuration(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let policies = input
        .get("LifecyclePolicies")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // AWS allows three transition keys per policy entry. Each accepts
    // a different enum set; reject values outside the documented list.
    const TRANSITION_DAYS: &[&str] = &[
        "AFTER_1_DAY",
        "AFTER_7_DAYS",
        "AFTER_14_DAYS",
        "AFTER_30_DAYS",
        "AFTER_60_DAYS",
        "AFTER_90_DAYS",
        "AFTER_180_DAYS",
        "AFTER_270_DAYS",
        "AFTER_365_DAYS",
        "NONE",
    ];
    const TRANSITION_PRIMARY: &[&str] = &["AFTER_1_ACCESS", "NONE"];
    for p in &policies {
        for (key, allowed) in [
            ("TransitionToIA", TRANSITION_DAYS),
            ("TransitionToArchive", TRANSITION_DAYS),
            ("TransitionToPrimaryStorageClass", TRANSITION_PRIMARY),
        ] {
            if let Some(v) = p.get(key).and_then(Value::as_str)
                && !allowed.contains(&v)
            {
                return Err(AwsError::bad_request(
                    "BadRequest",
                    format!("{key} `{v}` is not a valid LifecyclePolicy value."),
                ));
            }
        }
    }
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.lifecycle_policies = policies.clone();
    Ok(json!({ "LifecyclePolicies": policies }))
}

pub fn describe_lifecycle_configuration(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let fs = state.file_systems.get(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    Ok(json!({ "LifecyclePolicies": fs.lifecycle_policies }))
}

pub fn describe_backup_policy(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let fs = state.file_systems.get(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    Ok(json!({ "BackupPolicy": { "Status": fs.backup_policy_status } }))
}

pub fn put_backup_policy(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let status = input
        .get("BackupPolicy")
        .and_then(|p| p.get("Status"))
        .and_then(|v| v.as_str())
        .unwrap_or("ENABLED")
        .to_string();
    // AWS only accepts `ENABLED` and `DISABLED` from clients. The
    // transient `ENABLING` / `DISABLING` values appear in describe
    // responses while the lifecycle settles but are not valid inputs.
    if !matches!(status.as_str(), "ENABLED" | "DISABLED") {
        return Err(AwsError::bad_request(
            "BadRequest",
            format!("BackupPolicy.Status `{status}` must be ENABLED or DISABLED."),
        ));
    }
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.backup_policy_status = status.clone();
    Ok(json!({ "BackupPolicy": { "Status": status } }))
}

/// Attaches a resource policy JSON document to the file system. AWS
/// caps the document at 20 KiB and requires valid JSON;
/// `BypassPolicyLockoutSafetyCheck` is accepted as a no-op pending a
/// future heuristic.
pub fn put_file_system_policy(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let policy = input
        .get("Policy")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "Policy is required"))?
        .to_string();
    if policy.len() > 20 * 1024 {
        return Err(AwsError::bad_request(
            "PolicyTooLargeException",
            format!(
                "Policy document is {} bytes; AWS caps the document at 20 KiB.",
                policy.len(),
            ),
        ));
    }
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&policy) {
        return Err(AwsError::bad_request(
            "InvalidPolicyException",
            format!("Policy document is not valid JSON: {e}"),
        ));
    }
    let _bypass = input
        .get("BypassPolicyLockoutSafetyCheck")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.file_system_policy = Some(policy.clone());
    Ok(json!({
        "FileSystemId": fs.file_system_id,
        "Policy": policy,
    }))
}

/// Returns the previously-attached policy or PolicyNotFound.
pub fn describe_file_system_policy(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let fs = state.file_systems.get(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    let policy = fs.file_system_policy.clone().ok_or_else(|| {
        AwsError::not_found(
            "PolicyNotFound",
            format!("File system {id} has no attached policy."),
        )
    })?;
    Ok(json!({
        "FileSystemId": fs.file_system_id,
        "Policy": policy,
    }))
}

pub fn delete_file_system_policy(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.file_system_policy = None;
    Ok(json!({}))
}

/// Updates the `ReplicationOverwriteProtection` enum on a file system.
/// AWS allows `ENABLED` and `DISABLED`; the `REPLICATING` sentinel is
/// reserved for replica file systems set automatically by the
/// replication subsystem.
pub fn update_file_system_protection(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?;
    let value = input
        .get("ReplicationOverwriteProtection")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequest", "ReplicationOverwriteProtection is required")
        })?;
    if !matches!(value, "ENABLED" | "DISABLED") {
        return Err(AwsError::bad_request(
            "BadRequest",
            format!("ReplicationOverwriteProtection `{value}` must be ENABLED or DISABLED.",),
        ));
    }
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.file_system_protection_replication_overwrite_protection = value.to_string();
    Ok(json!({
        "FileSystemId": fs.file_system_id,
        "ReplicationOverwriteProtection": fs.file_system_protection_replication_overwrite_protection,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EfsState;

    fn ctx() -> RequestContext {
        RequestContext::new("efs", "us-east-1")
    }

    #[test]
    fn create_file_system_rejects_provisioned_throughput_without_mode() {
        let state = EfsState::default();
        let err = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-no-mode",
                "ProvisionedThroughputInMibps": 256.0,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_file_system_rejects_throughput_out_of_range() {
        let state = EfsState::default();
        let err = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-too-high",
                "ThroughputMode": "provisioned",
                "ProvisionedThroughputInMibps": 2048.0,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_file_system_requires_throughput_when_mode_provisioned() {
        let state = EfsState::default();
        let err = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-missing",
                "ThroughputMode": "provisioned",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn put_file_system_policy_round_trips_valid_json() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-pol" }), &ctx()).unwrap();
        let id = resp["FileSystemId"].as_str().unwrap().to_string();
        let policy = "{\"Version\":\"2012-10-17\",\"Statement\":[]}";
        put_file_system_policy(
            &state,
            &json!({ "FileSystemId": id, "Policy": policy }),
            &ctx(),
        )
        .unwrap();
        let described =
            describe_file_system_policy(&state, &json!({ "FileSystemId": id }), &ctx()).unwrap();
        assert_eq!(described["Policy"], policy);
        delete_file_system_policy(&state, &json!({ "FileSystemId": id }), &ctx()).unwrap();
        let err = describe_file_system_policy(&state, &json!({ "FileSystemId": id }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "PolicyNotFound");
    }

    #[test]
    fn put_file_system_policy_rejects_invalid_json() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-bad-json" }), &ctx()).unwrap();
        let id = resp["FileSystemId"].as_str().unwrap().to_string();
        let err = put_file_system_policy(
            &state,
            &json!({ "FileSystemId": id, "Policy": "not-json" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidPolicyException");
    }

    #[test]
    fn put_file_system_policy_rejects_oversized_document() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-large" }), &ctx()).unwrap();
        let id = resp["FileSystemId"].as_str().unwrap().to_string();
        let huge = format!("\"{}\"", "a".repeat(20 * 1024));
        let err = put_file_system_policy(
            &state,
            &json!({ "FileSystemId": id, "Policy": huge }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PolicyTooLargeException");
    }

    #[test]
    fn delete_file_system_rejects_when_access_points_exist() {
        use crate::operations::access_points::create_access_point;
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-del" }), &ctx()).unwrap();
        let id = resp["FileSystemId"].as_str().unwrap().to_string();
        create_access_point(
            &state,
            &json!({ "ClientToken": "ap-del", "FileSystemId": id }),
            &ctx(),
        )
        .unwrap();
        let err = delete_file_system(&state, &json!({ "FileSystemId": id }), &ctx()).unwrap_err();
        assert_eq!(err.code, "FileSystemInUse");
    }

    #[test]
    fn put_backup_policy_rejects_unknown_status() {
        let state = EfsState::default();
        let created =
            create_file_system(&state, &json!({ "CreationToken": "t-bp-bad" }), &ctx()).unwrap();
        let id = created["FileSystemId"].as_str().unwrap().to_string();
        let err = put_backup_policy(
            &state,
            &json!({
                "FileSystemId": id,
                "BackupPolicy": { "Status": "ENABLING" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn put_lifecycle_configuration_rejects_unknown_transition() {
        let state = EfsState::default();
        let created =
            create_file_system(&state, &json!({ "CreationToken": "t-lc-bad" }), &ctx()).unwrap();
        let id = created["FileSystemId"].as_str().unwrap().to_string();
        let err = put_lifecycle_configuration(
            &state,
            &json!({
                "FileSystemId": id,
                "LifecyclePolicies": [
                    { "TransitionToIA": "AFTER_2_DAYS" },
                ],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn put_lifecycle_configuration_accepts_documented_enums() {
        let state = EfsState::default();
        let created =
            create_file_system(&state, &json!({ "CreationToken": "t-lc-ok" }), &ctx()).unwrap();
        let id = created["FileSystemId"].as_str().unwrap().to_string();
        put_lifecycle_configuration(
            &state,
            &json!({
                "FileSystemId": id,
                "LifecyclePolicies": [
                    { "TransitionToIA": "AFTER_30_DAYS" },
                    { "TransitionToArchive": "AFTER_90_DAYS" },
                    { "TransitionToPrimaryStorageClass": "AFTER_1_ACCESS" },
                ],
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_file_system_persists_availability_zone_pin() {
        let state = EfsState::default();
        let resp = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-az",
                "AvailabilityZoneName": "us-east-1a",
                "AvailabilityZoneId": "use1-az1",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["AvailabilityZoneName"], "us-east-1a");
        assert_eq!(resp["AvailabilityZoneId"], "use1-az1");
    }

    #[test]
    fn create_file_system_emits_file_system_protection_block() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-protect" }), &ctx()).unwrap();
        assert_eq!(
            resp["FileSystemProtection"]["ReplicationOverwriteProtection"],
            "ENABLED"
        );
    }

    #[test]
    fn update_file_system_protection_toggles_value() {
        let state = EfsState::default();
        let created =
            create_file_system(&state, &json!({ "CreationToken": "t-toggle" }), &ctx()).unwrap();
        let id = created["FileSystemId"].as_str().unwrap().to_string();
        let resp = update_file_system_protection(
            &state,
            &json!({
                "FileSystemId": id,
                "ReplicationOverwriteProtection": "DISABLED",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["ReplicationOverwriteProtection"], "DISABLED");
    }

    #[test]
    fn update_file_system_protection_rejects_replicating_value() {
        let state = EfsState::default();
        let created =
            create_file_system(&state, &json!({ "CreationToken": "t-replicating" }), &ctx())
                .unwrap();
        let id = created["FileSystemId"].as_str().unwrap().to_string();
        let err = update_file_system_protection(
            &state,
            &json!({
                "FileSystemId": id,
                "ReplicationOverwriteProtection": "REPLICATING",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_file_system_emits_full_size_in_bytes_block() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-size" }), &ctx()).unwrap();
        let size = &resp["SizeInBytes"];
        assert_eq!(size["Value"], 0);
        assert_eq!(size["ValueInIA"], 0);
        assert_eq!(size["ValueInStandard"], 0);
        assert_eq!(size["ValueInArchive"], 0);
        assert!(size["Timestamp"].is_number());
    }

    #[test]
    fn create_file_system_defaults_kms_alias_when_encrypted() {
        let state = EfsState::default();
        let resp = create_file_system(
            &state,
            &json!({ "CreationToken": "t-enc", "Encrypted": true }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Encrypted"], true);
        assert_eq!(
            resp["KmsKeyId"],
            "arn:aws:kms:us-east-1:000000000000:alias/aws/elasticfilesystem"
        );
    }

    #[test]
    fn create_file_system_rejects_kms_key_when_not_encrypted() {
        let state = EfsState::default();
        let err = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-bad-kms",
                "Encrypted": false,
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_file_system_accepts_valid_provisioned_throughput() {
        let state = EfsState::default();
        let resp = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-ok",
                "ThroughputMode": "provisioned",
                "ProvisionedThroughputInMibps": 128.0,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["ThroughputMode"], "provisioned");
        assert_eq!(resp["ProvisionedThroughputInMibps"], 128.0);
    }

    #[test]
    fn replay_with_identical_args_returns_existing_file_system() {
        let state = EfsState::default();
        let body = json!({
            "CreationToken": "t-replay",
            "PerformanceMode": "generalPurpose",
            "ThroughputMode": "bursting",
        });
        let first = create_file_system(&state, &body, &ctx()).unwrap();
        let second = create_file_system(&state, &body, &ctx()).unwrap();
        assert_eq!(first["FileSystemId"], second["FileSystemId"]);
    }

    #[test]
    fn update_throughput_rejected_within_cooldown_window() {
        // SAFETY: tests in this module are single-threaded by default and
        // we restore the env var below.
        unsafe {
            std::env::remove_var("AWSIM_LIFECYCLE_FAST");
        }
        let state = EfsState::default();
        let id = create_file_system(&state, &json!({ "CreationToken": "t-cd" }), &ctx()).unwrap()
            ["FileSystemId"]
            .as_str()
            .unwrap()
            .to_string();
        // First change: bursting -> provisioned.
        update_file_system(
            &state,
            &json!({
                "FileSystemId": id,
                "ThroughputMode": "provisioned",
                "ProvisionedThroughputInMibps": 100.0,
            }),
            &ctx(),
        )
        .unwrap();
        // Second change inside the 24h window is rejected.
        let err = update_file_system(
            &state,
            &json!({
                "FileSystemId": id,
                "ProvisionedThroughputInMibps": 200.0,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ThroughputLimitExceeded");
    }

    #[test]
    fn awsim_lifecycle_fast_short_circuits_the_cooldown() {
        // SAFETY: see comment above; we reset the env var at end of test.
        unsafe {
            std::env::set_var("AWSIM_LIFECYCLE_FAST", "1");
        }
        let state = EfsState::default();
        let id = create_file_system(&state, &json!({ "CreationToken": "t-fast" }), &ctx()).unwrap()
            ["FileSystemId"]
            .as_str()
            .unwrap()
            .to_string();
        update_file_system(
            &state,
            &json!({
                "FileSystemId": id,
                "ThroughputMode": "provisioned",
                "ProvisionedThroughputInMibps": 100.0,
            }),
            &ctx(),
        )
        .unwrap();
        update_file_system(
            &state,
            &json!({
                "FileSystemId": id,
                "ProvisionedThroughputInMibps": 200.0,
            }),
            &ctx(),
        )
        .expect("second change should succeed when fast cooldown is enabled");
        unsafe {
            std::env::remove_var("AWSIM_LIFECYCLE_FAST");
        }
    }

    #[test]
    fn replay_with_mismatched_args_returns_file_system_already_exists() {
        let state = EfsState::default();
        create_file_system(
            &state,
            &json!({
                "CreationToken": "t-conflict",
                "PerformanceMode": "generalPurpose",
            }),
            &ctx(),
        )
        .unwrap();
        let err = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-conflict",
                "PerformanceMode": "maxIO",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "FileSystemAlreadyExists");
        assert!(err.message.contains("fs-"));
    }
}
