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
            "Timestamp": fs.creation_time,
        },
        "PerformanceMode": fs.performance_mode,
        "ThroughputMode": fs.throughput_mode,
        "ProvisionedThroughputInMibps": fs.provisioned_throughput_in_mibps,
        "Encrypted": fs.encrypted,
        "KmsKeyId": fs.kms_key_id,
        "Name": fs.name,
        "Tags": tags_to_array(&fs.tags),
        "OwnerId": "000000000000",
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

    // Idempotency: a fresh call with the same token returns the existing FS.
    if let Some(existing) = state
        .file_systems
        .iter()
        .find(|e| e.value().creation_token == token)
    {
        return Ok(fs_to_value(existing.value()));
    }

    let id = new_fs_id();
    let tags = tags_from_input(input);
    let name = tags
        .get("Name")
        .cloned()
        .or_else(|| input.get("Name").and_then(|v| v.as_str()).map(String::from));

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
        throughput_mode: input
            .get("ThroughputMode")
            .and_then(|v| v.as_str())
            .unwrap_or("bursting")
            .to_string(),
        provisioned_throughput_in_mibps: input
            .get("ProvisionedThroughputInMibps")
            .and_then(|v| v.as_f64()),
        encrypted: input
            .get("Encrypted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        kms_key_id: input
            .get("KmsKeyId")
            .and_then(|v| v.as_str())
            .map(String::from),
        name,
        tags,
        lifecycle_policies: vec![],
        backup_policy_status: "DISABLED".to_string(),
        file_system_protection_replication_overwrite_protection: "ENABLED".to_string(),
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

    let items: Vec<Value> = state
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
        .map(|e| fs_to_value(e.value()))
        .collect();

    Ok(json!({ "FileSystems": items }))
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
    if let Some(mode) = input.get("ThroughputMode").and_then(|v| v.as_str()) {
        fs.throughput_mode = mode.to_string();
    }
    if let Some(p) = input
        .get("ProvisionedThroughputInMibps")
        .and_then(|v| v.as_f64())
    {
        fs.provisioned_throughput_in_mibps = Some(p);
    }
    Ok(fs_to_value(&fs))
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
    let mut fs = state.file_systems.get_mut(id).ok_or_else(|| {
        AwsError::not_found("FileSystemNotFound", format!("File system {id} not found"))
    })?;
    fs.backup_policy_status = status.clone();
    Ok(json!({ "BackupPolicy": { "Status": status } }))
}
