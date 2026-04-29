use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BackupPlan, BackupState};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_plan_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn plan_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:backup:{}:{}:backup-plan:{}",
        ctx.region, ctx.account_id, id
    )
}

fn plan_to_value(p: &BackupPlan) -> Value {
    json!({
        "BackupPlanId": p.plan_id,
        "BackupPlanArn": p.plan_arn,
        "BackupPlanName": p.plan_name,
        "VersionId": p.version_id,
        "CreationDate": p.creation_date,
        "LastExecutionDate": p.last_execution_date,
    })
}

pub fn create_backup_plan(
    state: &BackupState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let plan = input.get("BackupPlan").ok_or_else(|| {
        AwsError::bad_request("InvalidParameterValueException", "BackupPlan is required")
    })?;
    let name = plan
        .get("BackupPlanName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "BackupPlanName is required",
            )
        })?
        .to_string();
    let rules = plan
        .get("Rules")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let id = new_plan_id();
    let tags: HashMap<String, String> = input
        .get("BackupPlanTags")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let p = BackupPlan {
        plan_id: id.clone(),
        plan_arn: plan_arn(ctx, &id),
        version_id: format!("{}_1", &id[..8]),
        plan_name: name,
        creation_date: now_secs(),
        last_execution_date: None,
        rules,
        advanced_settings: plan.get("AdvancedBackupSettings").cloned(),
        tags,
    };
    let result = json!({
        "BackupPlanId": p.plan_id,
        "BackupPlanArn": p.plan_arn,
        "CreationDate": p.creation_date,
        "VersionId": p.version_id,
    });
    state.plans.insert(id, p);
    Ok(result)
}

pub fn get_backup_plan(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let p = state.plans.get(id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("Plan {id} not found"))
    })?;
    Ok(json!({
        "BackupPlan": {
            "BackupPlanName": p.plan_name,
            "Rules": p.rules,
            "AdvancedBackupSettings": p.advanced_settings,
        },
        "BackupPlanId": p.plan_id,
        "BackupPlanArn": p.plan_arn,
        "VersionId": p.version_id,
        "CreationDate": p.creation_date,
        "LastExecutionDate": p.last_execution_date,
    }))
}

pub fn list_backup_plans(
    state: &BackupState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .plans
        .iter()
        .map(|e| plan_to_value(e.value()))
        .collect();
    Ok(json!({ "BackupPlansList": items }))
}

pub fn delete_backup_plan(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let (_, p) = state.plans.remove(id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("Plan {id} not found"))
    })?;
    // Cascade-delete selections for this plan.
    let to_remove: Vec<String> = state
        .selections
        .iter()
        .filter(|e| e.value().plan_id == id)
        .map(|e| e.key().clone())
        .collect();
    for k in to_remove {
        state.selections.remove(&k);
    }
    Ok(json!({
        "BackupPlanId": p.plan_id,
        "BackupPlanArn": p.plan_arn,
        "DeletionDate": now_secs(),
        "VersionId": p.version_id,
    }))
}

pub fn update_backup_plan(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let plan = input.get("BackupPlan").ok_or_else(|| {
        AwsError::bad_request("InvalidParameterValueException", "BackupPlan is required")
    })?;
    let mut p = state.plans.get_mut(id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("Plan {id} not found"))
    })?;
    if let Some(rules) = plan.get("Rules").and_then(|v| v.as_array()) {
        p.rules = rules.clone();
    }
    if let Some(name) = plan.get("BackupPlanName").and_then(|v| v.as_str()) {
        p.plan_name = name.to_string();
    }
    // Bump version
    let next: u32 = p
        .version_id
        .rsplit_once('_')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(1)
        + 1;
    p.version_id = format!("{}_{}", &p.plan_id[..8], next);
    Ok(json!({
        "BackupPlanId": p.plan_id,
        "BackupPlanArn": p.plan_arn,
        "VersionId": p.version_id,
    }))
}
