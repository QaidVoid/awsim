use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{
    SsmMaintenanceWindowTarget, SsmMaintenanceWindowTask, SsmResourceDataSync, SsmState,
};

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
        AwsError::bad_request(
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

    let mut window = state
        .maintenance_windows
        .get_mut(window_id)
        .ok_or_else(|| {
            AwsError::bad_request(
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
        return Err(AwsError::bad_request(
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
        return Err(AwsError::bad_request(
            "DoesNotExistException",
            format!("Maintenance window '{window_id}' does not exist"),
        ));
    }

    let task_arn = input["TaskArn"].as_str().unwrap_or("").to_string();
    let task_type = input["TaskType"]
        .as_str()
        .unwrap_or("RUN_COMMAND")
        .to_string();
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
        return Err(AwsError::bad_request(
            "ResourceDataSyncAlreadyExistsException",
            format!("Resource data sync '{sync_name}' already exists"),
        ));
    }

    let sync_type = input["SyncType"]
        .as_str()
        .unwrap_or("SyncToDestination")
        .to_string();
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
        .filter(|e| sync_type.is_none_or(|t| e.value().sync_type == t))
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
        return Err(AwsError::bad_request(
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
        AwsError::bad_request(
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
            AwsError::bad_request(
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

pub fn deregister_target_from_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"].as_str().unwrap_or("").to_string();
    let window_target_id = input["WindowTargetId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowTargetId is required"))?;

    state.maintenance_window_targets.remove(window_target_id);

    Ok(json!({
        "WindowId": window_id,
        "WindowTargetId": window_target_id,
    }))
}

pub fn deregister_task_from_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"].as_str().unwrap_or("").to_string();
    let window_task_id = input["WindowTaskId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowTaskId is required"))?;

    state.maintenance_window_tasks.remove(window_task_id);

    Ok(json!({
        "WindowId": window_id,
        "WindowTaskId": window_task_id,
    }))
}

pub fn get_maintenance_window_task(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_task_id = input["WindowTaskId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowTaskId is required"))?;

    let task = state
        .maintenance_window_tasks
        .get(window_task_id)
        .ok_or_else(|| {
            AwsError::bad_request(
                "DoesNotExistException",
                format!("Maintenance window task '{window_task_id}' not found"),
            )
        })?;

    Ok(json!({
        "WindowId": task.window_id,
        "WindowTaskId": task.window_task_id,
        "TaskArn": task.task_arn,
        "TaskType": task.task_type,
        "Targets": task.targets,
        "Priority": task.priority,
        "MaxConcurrency": task.max_concurrency,
        "MaxErrors": task.max_errors,
        "Name": task.name,
    }))
}

pub fn update_maintenance_window_target(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_target_id = input["WindowTargetId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowTargetId is required"))?;

    let mut t = state
        .maintenance_window_targets
        .get_mut(window_target_id)
        .ok_or_else(|| {
            AwsError::bad_request(
                "DoesNotExistException",
                format!("Maintenance window target '{window_target_id}' not found"),
            )
        })?;

    if let Some(targets) = input["Targets"].as_array() {
        t.targets = targets.clone();
    }
    if let Some(name) = input["Name"].as_str() {
        t.name = name.to_string();
    }

    Ok(json!({
        "WindowId": t.window_id,
        "WindowTargetId": t.window_target_id,
        "Targets": t.targets,
        "Name": t.name,
    }))
}

pub fn update_maintenance_window_task(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_task_id = input["WindowTaskId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowTaskId is required"))?;

    let mut t = state
        .maintenance_window_tasks
        .get_mut(window_task_id)
        .ok_or_else(|| {
            AwsError::bad_request(
                "DoesNotExistException",
                format!("Maintenance window task '{window_task_id}' not found"),
            )
        })?;

    if let Some(targets) = input["Targets"].as_array() {
        t.targets = targets.clone();
    }
    if let Some(p) = input["Priority"].as_u64() {
        t.priority = p;
    }
    if let Some(c) = input["MaxConcurrency"].as_str() {
        t.max_concurrency = c.to_string();
    }
    if let Some(e) = input["MaxErrors"].as_str() {
        t.max_errors = e.to_string();
    }

    Ok(json!({
        "WindowId": t.window_id,
        "WindowTaskId": t.window_task_id,
        "Targets": t.targets,
        "Priority": t.priority,
        "MaxConcurrency": t.max_concurrency,
        "MaxErrors": t.max_errors,
        "Name": t.name,
    }))
}

pub fn describe_instance_patches(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "InstanceId is required"))?;
    let _ = instance_id;
    Ok(json!({ "Patches": [] }))
}

pub fn describe_instance_patch_states(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids = input["InstanceIds"].as_array().cloned().unwrap_or_default();
    let out: Vec<Value> = ids
        .into_iter()
        .filter_map(|v| v.as_str().map(String::from))
        .map(|id| {
            json!({
                "InstanceId": id,
                "PatchGroup": "default",
                "BaselineId": "pb-default",
                "OperationStartTime": 0,
                "OperationEndTime": 0,
                "Operation": "Scan",
                "InstalledCount": 0,
                "MissingCount": 0,
                "FailedCount": 0,
                "NotApplicableCount": 0,
            })
        })
        .collect();
    Ok(json!({ "InstancePatchStates": out }))
}

pub fn describe_available_patches(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Patches": [] }))
}

pub fn describe_patch_groups(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Mappings": [] }))
}

pub fn describe_patch_group_state(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "Instances": 0,
        "InstancesWithInstalledPatches": 0,
        "InstancesWithInstalledOtherPatches": 0,
        "InstancesWithMissingPatches": 0,
        "InstancesWithFailedPatches": 0,
        "InstancesWithNotApplicablePatches": 0,
    }))
}

pub fn register_patch_baseline_for_patch_group(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"].as_str().unwrap_or("").to_string();
    let patch_group = input["PatchGroup"].as_str().unwrap_or("").to_string();
    Ok(json!({
        "BaselineId": baseline_id,
        "PatchGroup": patch_group,
    }))
}

pub fn deregister_patch_baseline_for_patch_group(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let baseline_id = input["BaselineId"].as_str().unwrap_or("").to_string();
    let patch_group = input["PatchGroup"].as_str().unwrap_or("").to_string();
    Ok(json!({
        "BaselineId": baseline_id,
        "PatchGroup": patch_group,
    }))
}

pub fn describe_instance_associations_status(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "InstanceAssociationStatusInfos": [] }))
}

pub fn describe_effective_instance_associations(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Associations": [] }))
}

pub fn describe_association_executions(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "AssociationExecutions": [] }))
}

pub fn describe_association_execution_targets(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "AssociationExecutionTargets": [] }))
}

pub fn list_association_versions(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let association_id = input["AssociationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AssociationId is required"))?;

    let versions: Vec<Value> = state
        .associations
        .get(association_id)
        .map(|a| {
            vec![json!({
                "AssociationId": a.association_id,
                "AssociationVersion": "1",
                "CreatedDate": a.created_date,
                "Name": a.document_name,
                "Targets": a.targets,
            })]
        })
        .unwrap_or_default();

    Ok(json!({ "AssociationVersions": versions }))
}

pub fn update_service_setting(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let setting_id = input["SettingId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SettingId is required"))?
        .to_string();
    let setting_value = input["SettingValue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SettingValue is required"))?
        .to_string();

    state.service_settings.insert(setting_id, setting_value);
    Ok(json!({}))
}

pub fn reset_service_setting(
    state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let setting_id = input["SettingId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SettingId is required"))?;

    state.service_settings.remove(setting_id);

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

pub fn describe_automation_step_executions(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "StepExecutions": [] }))
}

pub fn send_automation_signal(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn cancel_command(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let command_id = input["CommandId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "CommandId is required"))?;

    if let Some(mut c) = state.commands.get_mut(command_id) {
        c.status = "Cancelled".to_string();
    }

    Ok(json!({}))
}

pub fn list_command_invocations(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let command_id = input["CommandId"].as_str();
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let invocations: Vec<Value> = state
        .commands
        .iter()
        .filter(|e| command_id.is_none_or(|id| e.command_id == id))
        .map(|e| {
            let c = e.value();
            json!({
                "CommandId": c.command_id,
                "InstanceId": "i-0000000000000000",
                "DocumentName": c.document_name,
                "Status": c.status,
                "RequestedDateTime": c.created_time,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "CommandInvocations": invocations }))
}

pub fn unlabel_parameter_version(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;
    let labels = input["Labels"].as_array().cloned().unwrap_or_default();
    let labels: Vec<String> = labels
        .into_iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let version = input["ParameterVersion"].as_u64().unwrap_or(0);

    let mut removed = Vec::new();
    if let Some(mut p) = state.parameters.get_mut(name) {
        if version == p.version || version == 0 {
            p.labels.retain(|l| {
                if labels.contains(l) {
                    removed.push(l.clone());
                    false
                } else {
                    true
                }
            });
        }
        for entry in p.history.iter_mut() {
            if entry.version == version {
                entry.labels.retain(|l| !labels.contains(l));
            }
        }
    }

    Ok(json!({
        "RemovedLabels": removed,
        "InvalidLabels": [],
    }))
}

pub fn delete_inventory(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "DeletionId": uuid::Uuid::new_v4().to_string() }))
}

pub fn update_resource_data_sync(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sync_name = input["SyncName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SyncName is required"))?;

    if !state.resource_data_syncs.contains_key(sync_name) {
        return Err(AwsError::bad_request(
            "ResourceDataSyncNotFoundException",
            format!("Resource data sync '{sync_name}' does not exist"),
        ));
    }
    Ok(json!({}))
}

pub fn get_connection_status(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let target = input["Target"].as_str().unwrap_or("");
    Ok(json!({
        "Target": target,
        "Status": "connected",
    }))
}

pub fn get_calendar_state(
    _state: &SsmState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "State": "OPEN",
        "AtTime": "2024-01-01T00:00:00Z",
        "NextTransitionTime": "2024-01-02T00:00:00Z",
    }))
}

pub fn update_document_default_version(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;
    let version = input["DocumentVersion"].as_str().unwrap_or("1").to_string();

    if let Some(mut d) = state.documents.get_mut(name) {
        d.document_version = version.clone();
    }

    Ok(json!({
        "Description": {
            "Name": name,
            "DefaultVersion": version,
        }
    }))
}

pub fn list_document_versions(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let versions: Vec<Value> = state
        .documents
        .get(name)
        .map(|d| {
            vec![json!({
                "Name": d.name,
                "DocumentVersion": d.document_version,
                "CreatedDate": d.created_date,
                "IsDefaultVersion": true,
                "DocumentFormat": d.document_format,
                "Status": d.status,
            })]
        })
        .unwrap_or_default();

    Ok(json!({ "DocumentVersions": versions }))
}
