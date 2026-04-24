use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AccountAssignment, SsoAdminState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn create_account_assignment(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_arn = input["InstanceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "InstanceArn is required"))?
        .to_string();
    let permission_set_arn = input["PermissionSetArn"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "PermissionSetArn is required")
        })?
        .to_string();
    let target_id = input["TargetId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "TargetId is required"))?
        .to_string();
    let target_type = input["TargetType"]
        .as_str()
        .unwrap_or("AWS_ACCOUNT")
        .to_string();
    let principal_id = input["PrincipalId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "PrincipalId is required"))?
        .to_string();
    let principal_type = input["PrincipalType"]
        .as_str()
        .unwrap_or("USER")
        .to_string();

    let id = format!("aa-{}", uuid::Uuid::new_v4().simple());
    let assignment = AccountAssignment {
        id: id.clone(),
        instance_arn,
        permission_set_arn,
        account_id: target_id.clone(),
        principal_id,
        principal_type,
        status: "SUCCEEDED".to_string(),
        target_type,
        target_id,
        requested_at: now_secs(),
        request_type: "CREATE".to_string(),
    };

    state
        .account_assignments
        .insert(id.clone(), assignment.clone());

    Ok(json!({
        "AccountAssignmentCreationStatus": {
            "Status": assignment.status,
            "RequestId": id,
            "FailureReason": "",
            "TargetId": assignment.target_id,
            "TargetType": assignment.target_type,
            "PermissionSetArn": assignment.permission_set_arn,
            "PrincipalType": assignment.principal_type,
            "PrincipalId": assignment.principal_id,
            "CreatedDate": assignment.requested_at,
        }
    }))
}

pub fn describe_account_assignment_creation_status(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["AccountAssignmentCreationRequestId"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "ValidationException",
                "AccountAssignmentCreationRequestId is required",
            )
        })?;

    let a = state.account_assignments.get(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Assignment not found: {id}"),
        )
    })?;

    Ok(json!({
        "AccountAssignmentCreationStatus": {
            "Status": a.status,
            "RequestId": a.id,
            "FailureReason": "",
            "TargetId": a.target_id,
            "TargetType": a.target_type,
            "PermissionSetArn": a.permission_set_arn,
            "PrincipalType": a.principal_type,
            "PrincipalId": a.principal_id,
            "CreatedDate": a.requested_at,
        }
    }))
}

pub fn list_account_assignments(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let account_id_filter = input["AccountId"].as_str();
    let ps_arn_filter = input["PermissionSetArn"].as_str();

    let list: Vec<Value> = state
        .account_assignments
        .iter()
        .filter(|e| {
            let a = e.value();
            account_id_filter.is_none_or(|f| a.account_id == f)
                && ps_arn_filter.is_none_or(|f| a.permission_set_arn == f)
        })
        .map(|e| {
            let a = e.value();
            json!({
                "AccountId": a.account_id,
                "PermissionSetArn": a.permission_set_arn,
                "PrincipalId": a.principal_id,
                "PrincipalType": a.principal_type,
            })
        })
        .collect();

    Ok(json!({ "AccountAssignments": list }))
}

pub fn delete_account_assignment(
    state: &SsoAdminState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ps_arn = input["PermissionSetArn"].as_str().unwrap_or("");
    let target_id = input["TargetId"].as_str().unwrap_or("");
    let principal_id = input["PrincipalId"].as_str().unwrap_or("");

    let keys: Vec<String> = state
        .account_assignments
        .iter()
        .filter(|e| {
            let a = e.value();
            a.permission_set_arn == ps_arn
                && a.target_id == target_id
                && a.principal_id == principal_id
        })
        .map(|e| e.key().clone())
        .collect();

    for k in &keys {
        state.account_assignments.remove(k);
    }

    let id = format!("aa-{}", uuid::Uuid::new_v4().simple());
    Ok(json!({
        "AccountAssignmentDeletionStatus": {
            "Status": "SUCCEEDED",
            "RequestId": id,
            "TargetId": target_id,
            "TargetType": input["TargetType"].as_str().unwrap_or("AWS_ACCOUNT"),
            "PermissionSetArn": ps_arn,
            "PrincipalType": input["PrincipalType"].as_str().unwrap_or("USER"),
            "PrincipalId": principal_id,
            "CreatedDate": now_secs(),
        }
    }))
}
