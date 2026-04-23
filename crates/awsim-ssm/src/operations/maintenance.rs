use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmMaintenanceWindowTarget, SsmMaintenanceWindowTask, SsmResourceDataSync, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn get_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowId is required"))?;

    let window = state.maintenance_windows.get(window_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Maintenance window '{window_id}' does not exist"),
        )
    })?;

    Ok(json!({
        "WindowId": window.window_id,
        "Name": window.name,
        "Description": "",
        "Schedule": window.schedule,
        "Duration": window.duration,
        "Cutoff": window.cutoff,
        "AllowUnassociatedTargets": false,
        "Enabled": window.enabled,
        "CreatedDate": window.created_date,
        "ModifiedDate": window.created_date,
    }))
}

pub fn update_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowId is required"))?;

    let mut window = state.maintenance_windows.get_mut(window_id).ok_or_else(|| {
        AwsError::not_found(
            "DoesNotExistException",
            format!("Maintenance window '{window_id}' does not exist"),
        )
    })?;

    if let Some(name) = input["Name"].as_str() {
        window.name = name.to_string();
    }
    if let Some(schedule) = input["Schedule"].as_str() {
        window.schedule = schedule.to_string();
    }
    if let Some(duration) = input["Duration"].as_u64() {
        window.duration = duration;
    }
    if let Some(cutoff) = input["Cutoff"].as_u64() {
        window.cutoff = cutoff;
    }
    if let Some(enabled) = input["Enabled"].as_bool() {
        window.enabled = enabled;
    }

    Ok(json!({
        "WindowId": window.window_id,
        "Name": window.name,
        "Schedule": window.schedule,
        "Duration": window.duration,
        "Cutoff": window.cutoff,
        "Enabled": window.enabled,
        "AllowUnassociatedTargets": false,
    }))
}

pub fn register_target_with_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowId is required"))?
        .to_string();

    if !state.maintenance_windows.contains_key(&window_id) {
        return Err(AwsError::not_found(
            "DoesNotExistException",
            format!("Maintenance window '{window_id}' does not exist"),
        ));
    }

    let resource_type = input["ResourceType"]
        .as_str()
        .unwrap_or("INSTANCE")
        .to_string();
    let targets = input["Targets"].as_array().cloned().unwrap_or_default();
    let name = input["Name"].as_str().unwrap_or("").to_string();

    let window_target_id = Uuid::new_v4().to_string();
    let target = SsmMaintenanceWindowTarget {
        window_target_id: window_target_id.clone(),
        window_id,
        resource_type,
        targets,
        name,
    };

    state
        .maintenance_window_targets
        .insert(window_target_id.clone(), target);

    Ok(json!({ "WindowTargetId": window_target_id }))
}

pub fn register_task_with_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowId is required"))?
        .to_string();

    if !state.maintenance_windows.contains_key(&window_id) {
        return Err(AwsError::not_found(
            "DoesNotExistException",
            format!("Maintenance window '{window_id}' does not exist"),
        ));
    }

    let task_arn = input["TaskArn"].as_str().unwrap_or("").to_string();
    let task_type = input["TaskType"].as_str().unwrap_or("RUN_COMMAND").to_string();
    let targets = input["Targets"].as_array().cloned().unwrap_or_default();
    let priority = input["Priority"].as_u64().unwrap_or(1);
    let max_concurrency = input["MaxConcurrency"].as_str().unwrap_or("1").to_string();
    let max_errors = input["MaxErrors"].as_str().unwrap_or("0").to_string();
    let name = input["Name"].as_str().unwrap_or("").to_string();

    let window_task_id = Uuid::new_v4().to_string();
    let task = SsmMaintenanceWindowTask {
        window_task_id: window_task_id.clone(),
        window_id,
        task_arn,
        task_type,
        targets,
        priority,
        max_concurrency,
        max_errors,
        name,
    };

    state
        .maintenance_window_tasks
        .insert(window_task_id.clone(), task);

    Ok(json!({ "WindowTaskId": window_task_id }))
}

pub fn describe_maintenance_window_targets(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"].as_str().unwrap_or("");

    let targets: Vec<Value> = state
        .maintenance_window_targets
        .iter()
        .filter(|e| window_id.is_empty() || e.value().window_id == window_id)
        .map(|e| {
            let t = e.value();
            json!({
                "WindowId": t.window_id,
                "WindowTargetId": t.window_target_id,
                "ResourceType": t.resource_type,
                "Targets": t.targets,
                "Name": t.name,
            })
        })
        .collect();

    Ok(json!({ "Targets": targets }))
}

pub fn describe_maintenance_window_tasks(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"].as_str().unwrap_or("");

    let tasks: Vec<Value> = state
        .maintenance_window_tasks
        .iter()
        .filter(|e| window_id.is_empty() || e.value().window_id == window_id)
        .map(|e| {
            let t = e.value();
            json!({
                "WindowId": t.window_id,
                "WindowTaskId": t.window_task_id,
                "TaskArn": t.task_arn,
                "Type": t.task_type,
                "Targets": t.targets,
                "Priority": t.priority,
                "MaxConcurrency": t.max_concurrency,
                "MaxErrors": t.max_errors,
                "Name": t.name,
            })
        })
        .collect();

    Ok(json!({ "Tasks": tasks }))
}

pub fn create_resource_data_sync(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sync_name = input["SyncName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SyncName is required"))?
        .to_string();

    if state.resource_data_syncs.contains_key(&sync_name) {
        return Err(AwsError::conflict(
            "ResourceDataSyncAlreadyExistsException",
            format!("Resource data sync '{sync_name}' already exists"),
        ));
    }

    let sync_type = input["SyncType"].as_str().unwrap_or("SyncToDestination").to_string();
    let s3 = &input["S3Destination"];
    let s3_bucket_name = s3["BucketName"].as_str().unwrap_or("").to_string();
    let s3_region = s3["Region"].as_str().unwrap_or("us-east-1").to_string();
    let s3_prefix = s3["Prefix"].as_str().unwrap_or("").to_string();

    let now = now_epoch_secs();
    let sync = SsmResourceDataSync {
        sync_name: sync_name.clone(),
        sync_type,
        s3_bucket_name,
        s3_region,
        s3_prefix,
        last_sync_time: now,
        sync_created_time: now,
    };

    state.resource_data_syncs.insert(sync_name, sync);

    Ok(json!({}))
}

pub fn list_resource_data_sync(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sync_type = input["SyncType"].as_str();

    let items: Vec<Value> = state
        .resource_data_syncs
        .iter()
        .filter(|e| sync_type.map_or(true, |t| e.value().sync_type == t))
        .map(|e| {
            let s = e.value();
            json!({
                "SyncName": s.sync_name,
                "SyncType": s.sync_type,
                "S3Destination": {
                    "BucketName": s.s3_bucket_name,
                    "Region": s.s3_region,
                    "Prefix": s.s3_prefix,
                    "SyncFormat": "JsonSerDe",
                },
                "LastSyncTime": s.last_sync_time,
                "LastSuccessfulSyncTime": s.last_sync_time,
                "SyncCreatedTime": s.sync_created_time,
            })
        })
        .collect();

    Ok(json!({ "ResourceDataSyncItems": items }))
}

pub fn delete_resource_data_sync(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sync_name = input["SyncName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SyncName is required"))?;

    if state.resource_data_syncs.remove(sync_name).is_none() {
        return Err(AwsError::not_found(
            "ResourceDataSyncNotFoundException",
            format!("Resource data sync '{sync_name}' does not exist"),
        ));
    }

    Ok(json!({}))
}

pub fn update_association(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let association_id = input["AssociationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AssociationId is required"))?;

    let mut assoc = state.associations.get_mut(association_id).ok_or_else(|| {
        AwsError::not_found(
            "AssociationDoesNotExist",
            format!("Association '{association_id}' does not exist"),
        )
    })?;

    if let Some(name) = input["Name"].as_str() {
        assoc.document_name = name.to_string();
    }
    if let Some(targets) = input["Targets"].as_array() {
        assoc.targets = targets.clone();
    }

    Ok(json!({
        "AssociationDescription": {
            "AssociationId": assoc.association_id,
            "Name": assoc.document_name,
            "Targets": assoc.targets,
            "Status": { "Name": assoc.status },
            "Date": assoc.created_date,
        }
    }))
}

pub fn update_association_status(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;
    let instance_id = input["InstanceId"].as_str().unwrap_or("");

    let assoc = state
        .associations
        .iter()
        .find(|e| e.value().document_name == name)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "AssociationDoesNotExist",
                format!("Association for '{name}' does not exist"),
            )
        })?;

    Ok(json!({
        "AssociationDescription": {
            "AssociationId": assoc.association_id,
            "Name": assoc.document_name,
            "InstanceId": instance_id,
            "Status": input["AssociationStatus"].clone(),
            "Targets": assoc.targets,
            "Date": assoc.created_date,
        }
    }))
}

pub fn get_service_setting(
    _state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let setting_id = input["SettingId"].as_str().unwrap_or("/ssm/default");

    Ok(json!({
        "ServiceSetting": {
            "SettingId": setting_id,
            "SettingValue": "false",
            "LastModifiedDate": now_epoch_secs(),
            "LastModifiedUser": "awsim",
            "ARN": format!("arn:aws:ssm:{}:{}:servicesetting{}", ctx.region, ctx.account_id, setting_id),
            "Status": "Default",
        }
    }))
}

pub fn list_inventory_entries(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"].as_str().unwrap_or("i-00000000");
    let type_name = input["TypeName"].as_str().unwrap_or("AWS:Application");

    Ok(json!({
        "TypeName": type_name,
        "InstanceId": instance_id,
        "SchemaVersion": "1.0",
        "CaptureTime": "2024-01-01T00:00:00Z",
        "Entries": [],
    }))
}

pub fn list_compliance_summaries(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "ComplianceSummaryItems": [] }))
}
