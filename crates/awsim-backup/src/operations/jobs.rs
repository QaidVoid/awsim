use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BackupJob, BackupState};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_job_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn resource_type_from_arn(arn: &str) -> String {
    // Best-effort heuristic — the real service infers this from the ARN prefix.
    if arn.contains(":dynamodb:") {
        "DynamoDB".into()
    } else if arn.contains(":s3:") {
        "S3".into()
    } else if arn.contains(":efs:") || arn.contains(":elasticfilesystem:") {
        "EFS".into()
    } else if arn.contains(":rds:") {
        "RDS".into()
    } else if arn.contains(":ec2:") {
        "EBS".into()
    } else {
        "Unknown".into()
    }
}

fn job_to_value(j: &BackupJob) -> Value {
    json!({
        "BackupJobId": j.job_id,
        "BackupVaultName": j.backup_vault_name,
        "BackupVaultArn": j.backup_vault_arn,
        "RecoveryPointArn": j.recovery_point_arn,
        "ResourceArn": j.resource_arn,
        "CreationDate": j.creation_date,
        "CompletionDate": j.completion_date,
        "State": j.state,
        "StatusMessage": j.status_message,
        "PercentDone": j.percent_done,
        "BackupSizeInBytes": j.backup_size_in_bytes,
        "IamRoleArn": j.iam_role_arn,
        "ResourceType": j.resource_type,
    })
}

pub fn start_backup_job(
    state: &BackupState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let vault_name = input
        .get("BackupVaultName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupVaultName is required",
            )
        })?
        .to_string();
    let resource_arn = input
        .get("ResourceArn")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "ResourceArn is required")
        })?
        .to_string();
    let iam_role_arn = input
        .get("IamRoleArn")
        .and_then(|v| v.as_str())
        .unwrap_or("arn:aws:iam::000000000000:role/BackupRole")
        .to_string();

    let vault = state.vaults.get(&vault_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault_name} not found"),
        )
    })?;
    let vault_arn = vault.arn.clone();
    drop(vault);

    let job_id = new_job_id();
    let now = now_secs();
    let recovery_point_arn = format!(
        "arn:aws:backup:{}:{}:recovery-point:{}",
        ctx.region, ctx.account_id, &job_id
    );
    // Emulator collapses the queued/running cycle — jobs land in COMPLETED
    // immediately so callers don't have to poll. The recovery-point bookkeeping
    // is kept light: count it and move on.
    let job = BackupJob {
        job_id: job_id.clone(),
        backup_vault_name: vault_name.clone(),
        backup_vault_arn: vault_arn,
        recovery_point_arn: recovery_point_arn.clone(),
        resource_arn: resource_arn.clone(),
        creation_date: now,
        completion_date: Some(now),
        state: "COMPLETED".to_string(),
        status_message: None,
        percent_done: "100".to_string(),
        backup_size_in_bytes: 0,
        iam_role_arn,
        resource_type: resource_type_from_arn(&resource_arn),
    };
    state.jobs.insert(job_id.clone(), job);
    if let Some(mut v) = state.vaults.get_mut(&vault_name) {
        v.number_of_recovery_points += 1;
    }
    Ok(json!({
        "BackupJobId": job_id,
        "RecoveryPointArn": recovery_point_arn,
        "CreationDate": now,
    }))
}

pub fn describe_backup_job(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("BackupJobId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupJobId is required")
        })?;
    let j = state.jobs.get(id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("Job {id} not found"))
    })?;
    Ok(job_to_value(&j))
}

pub fn list_backup_jobs(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let vault_filter = input.get("ByBackupVaultName").and_then(|v| v.as_str());
    let state_filter = input.get("ByState").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .jobs
        .iter()
        .filter(|e| {
            if let Some(v) = vault_filter
                && e.value().backup_vault_name != v
            {
                return false;
            }
            if let Some(s) = state_filter
                && e.value().state != s
            {
                return false;
            }
            true
        })
        .map(|e| job_to_value(e.value()))
        .collect();
    Ok(json!({ "BackupJobs": items }))
}
