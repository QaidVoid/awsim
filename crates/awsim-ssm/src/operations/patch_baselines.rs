use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmPatchBaseline, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_baseline_id() -> String {
    format!(
        "pb-{}",
        Uuid::new_v4().to_string().replace('-', "")[..17].to_string()
    )
}

fn baseline_identity(b: &SsmPatchBaseline) -> Value {
    json!({
        "BaselineId": b.baseline_id,
        "BaselineName": b.name,
        "OperatingSystem": b.operating_system,
        "BaselineDescription": b.description,
        "DefaultBaseline": false,
    })
}

pub fn create_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?
        .to_string();

    let operating_system = input["OperatingSystem"]
        .as_str()
        .unwrap_or("WINDOWS")
        .to_string();
    let description = input["Description"].as_str().unwrap_or("").to_string();

    let approved_patches: Vec<String> = input["ApprovedPatches"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let rejected_patches: Vec<String> = input["RejectedPatches"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let baseline_id = new_baseline_id();
    let now = now_epoch_secs();

    let baseline = SsmPatchBaseline {
        baseline_id: baseline_id.clone(),
        name,
        operating_system,
        description,
        approved_patches,
        rejected_patches,
        created_date: now,
        modified_date: now,
    };

    state.patch_baselines.insert(baseline_id.clone(), baseline);

    Ok(json!({ "BaselineId": baseline_id }))
}

pub fn get_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "BaselineId is required"))?;

    let baseline = state.patch_baselines.get(baseline_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Patch baseline '{baseline_id}' does not exist"),
        )
    })?;

    Ok(json!({
        "BaselineId": baseline.baseline_id,
        "Name": baseline.name,
        "OperatingSystem": baseline.operating_system,
        "Description": baseline.description,
        "ApprovedPatches": baseline.approved_patches,
        "RejectedPatches": baseline.rejected_patches,
        "GlobalFilters": { "PatchFilters": [] },
        "ApprovalRules": { "PatchRules": [] },
        "PatchGroups": [],
        "CreatedDate": baseline.created_date,
        "ModifiedDate": baseline.modified_date,
        "Sources": [],
    }))
}

pub fn describe_patch_baselines(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let baselines: Vec<Value> = state
        .patch_baselines
        .iter()
        .map(|e| baseline_identity(e.value()))
        .take(max_results)
        .collect();

    Ok(json!({ "BaselineIdentities": baselines }))
}

pub fn delete_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "BaselineId is required"))?;

    if state.patch_baselines.remove(baseline_id).is_none() {
        return Err(AwsError::not_found(
            "DoesNotExistException",
            format!("Patch baseline '{baseline_id}' does not exist"),
        ));
    }

    Ok(json!({ "BaselineId": baseline_id }))
}

pub fn update_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "BaselineId is required"))?;

    let mut baseline = state.patch_baselines.get_mut(baseline_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Patch baseline '{baseline_id}' does not exist"),
        )
    })?;

    if let Some(name) = input["Name"].as_str() {
        baseline.name = name.to_string();
    }
    if let Some(description) = input["Description"].as_str() {
        baseline.description = description.to_string();
    }
    if let Some(arr) = input["ApprovedPatches"].as_array() {
        baseline.approved_patches = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(arr) = input["RejectedPatches"].as_array() {
        baseline.rejected_patches = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    baseline.modified_date = now_epoch_secs();

    Ok(json!({
        "BaselineId": baseline.baseline_id,
        "Name": baseline.name,
        "OperatingSystem": baseline.operating_system,
        "Description": baseline.description,
        "ApprovedPatches": baseline.approved_patches,
        "RejectedPatches": baseline.rejected_patches,
        "CreatedDate": baseline.created_date,
        "ModifiedDate": baseline.modified_date,
    }))
}

pub fn register_default_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "BaselineId is required"))?;

    let os = state
        .patch_baselines
        .get(baseline_id)
        .map(|b| b.operating_system.clone())
        .unwrap_or_else(|| "WINDOWS".to_string());

    state
        .default_patch_baselines
        .insert(os, baseline_id.to_string());

    Ok(json!({ "BaselineId": baseline_id }))
}

pub fn get_default_patch_baseline(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let os = input["OperatingSystem"]
        .as_str()
        .unwrap_or("WINDOWS")
        .to_string();

    let baseline_id = state
        .default_patch_baselines
        .get(&os)
        .map(|v| v.value().clone())
        .unwrap_or_else(|| {
            format!(
                "arn:aws:ssm:us-east-1::patchbaseline/pb-default-{}",
                os.to_lowercase()
            )
        });

    Ok(json!({
        "BaselineId": baseline_id,
        "OperatingSystem": os,
    }))
}
