use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BackupState, BackupVault};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn vault_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:backup:{}:{}:backup-vault:{}",
        ctx.region, ctx.account_id, name
    )
}

fn vault_to_value(v: &BackupVault) -> Value {
    json!({
        "BackupVaultName": v.name,
        "BackupVaultArn": v.arn,
        "CreationDate": v.creation_date,
        "EncryptionKeyArn": v.encryption_key_arn,
        "CreatorRequestId": v.creator_request_id,
        "NumberOfRecoveryPoints": v.number_of_recovery_points,
        "Locked": v.locked,
        "MinRetentionDays": v.min_retention_days,
        "MaxRetentionDays": v.max_retention_days,
    })
}

pub fn create_backup_vault(
    state: &BackupState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?
        .to_string();
    if state.vaults.contains_key(&name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Vault {name} already exists"),
        ));
    }
    let tags: HashMap<String, String> = input
        .get("BackupVaultTags")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let v = BackupVault {
        name: name.clone(),
        arn: vault_arn(ctx, &name),
        creation_date: now_secs(),
        encryption_key_arn: input
            .get("EncryptionKeyArn")
            .and_then(|v| v.as_str())
            .map(String::from),
        creator_request_id: input
            .get("CreatorRequestId")
            .and_then(|v| v.as_str())
            .map(String::from),
        number_of_recovery_points: 0,
        locked: false,
        min_retention_days: None,
        max_retention_days: None,
        tags,
    };
    let result = json!({
        "BackupVaultName": v.name,
        "BackupVaultArn": v.arn,
        "CreationDate": v.creation_date,
    });
    state.vaults.insert(name, v);
    Ok(result)
}

pub fn describe_backup_vault(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?;
    let v = state.vaults.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {name} not found"),
        )
    })?;
    Ok(vault_to_value(&v))
}

pub fn list_backup_vaults(
    state: &BackupState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .vaults
        .iter()
        .map(|e| vault_to_value(e.value()))
        .collect();
    Ok(json!({ "BackupVaultList": items }))
}

pub fn delete_backup_vault(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?;
    let (_, v) = state.vaults.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {name} not found"),
        )
    })?;
    if v.number_of_recovery_points > 0 {
        // Restore and reject — real Backup blocks delete on a non-empty vault.
        state.vaults.insert(v.name.clone(), v);
        return Err(AwsError::bad_request(
            "InvalidRequestException",
            "Cannot delete a vault that has recovery points",
        ));
    }
    Ok(json!({}))
}

pub fn put_backup_vault_lock_configuration(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?;
    let mut v = state.vaults.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {name} not found"),
        )
    })?;
    v.locked = true;
    v.min_retention_days = input
        .get("MinRetentionDays")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    v.max_retention_days = input
        .get("MaxRetentionDays")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    Ok(json!({}))
}

pub fn delete_backup_vault_lock_configuration(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?;
    let mut v = state.vaults.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {name} not found"),
        )
    })?;
    v.locked = false;
    v.min_retention_days = None;
    v.max_retention_days = None;
    Ok(json!({}))
}
