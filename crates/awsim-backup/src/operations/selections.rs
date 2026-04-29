use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BackupSelection, BackupState};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_selection_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn selection_to_value(s: &BackupSelection) -> Value {
    json!({
        "SelectionId": s.selection_id,
        "BackupPlanId": s.plan_id,
        "SelectionName": s.selection_name,
        "IamRoleArn": s.iam_role_arn,
        "Resources": s.resources,
        "ListOfTags": s.list_of_tags,
        "Conditions": s.conditions,
        "CreationDate": s.creation_date,
    })
}

pub fn create_backup_selection(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let plan_id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?
        .to_string();
    if !state.plans.contains_key(&plan_id) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Plan {plan_id} not found"),
        ));
    }
    let sel = input.get("BackupSelection").ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValueException",
            "BackupSelection is required",
        )
    })?;

    let id = new_selection_id();
    let s = BackupSelection {
        selection_id: id.clone(),
        plan_id: plan_id.clone(),
        selection_name: sel
            .get("SelectionName")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string(),
        iam_role_arn: sel
            .get("IamRoleArn")
            .and_then(|v| v.as_str())
            .unwrap_or("arn:aws:iam::000000000000:role/BackupRole")
            .to_string(),
        resources: sel
            .get("Resources")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        list_of_tags: sel
            .get("ListOfTags")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        conditions: sel.get("Conditions").cloned(),
        creation_date: now_secs(),
    };
    let result = json!({
        "SelectionId": s.selection_id,
        "BackupPlanId": s.plan_id,
        "CreationDate": s.creation_date,
    });
    let key = format!("{plan_id}:{id}");
    state.selections.insert(key, s);
    Ok(result)
}

pub fn get_backup_selection(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let plan_id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let sel_id = input
        .get("SelectionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "SelectionId is required")
        })?;
    let key = format!("{plan_id}:{sel_id}");
    let s = state.selections.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Selection {sel_id} not found"),
        )
    })?;
    Ok(json!({
        "BackupSelection": {
            "SelectionName": s.selection_name,
            "IamRoleArn": s.iam_role_arn,
            "Resources": s.resources,
            "ListOfTags": s.list_of_tags,
            "Conditions": s.conditions,
        },
        "SelectionId": s.selection_id,
        "BackupPlanId": s.plan_id,
        "CreationDate": s.creation_date,
    }))
}

pub fn list_backup_selections(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let plan_id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let items: Vec<Value> = state
        .selections
        .iter()
        .filter(|e| e.value().plan_id == plan_id)
        .map(|e| selection_to_value(e.value()))
        .collect();
    Ok(json!({ "BackupSelectionsList": items }))
}

pub fn delete_backup_selection(
    state: &BackupState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let plan_id = input
        .get("BackupPlanId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "BackupPlanId is required")
        })?;
    let sel_id = input
        .get("SelectionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValueException", "SelectionId is required")
        })?;
    let key = format!("{plan_id}:{sel_id}");
    state.selections.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Selection {sel_id} not found"),
        )
    })?;
    Ok(json!({}))
}
