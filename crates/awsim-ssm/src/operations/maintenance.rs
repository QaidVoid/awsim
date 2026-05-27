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

/// Maximum number of entries AWS accepts in a maintenance-window
/// `Targets` list. Real AWS bounces oversize requests with
/// `InvalidParameters`.
const MAX_TARGETS: usize = 50;

/// Validate one maintenance-window Target entry.
///
/// Each entry must be a JSON object with:
/// - `Key` set to one of `InstanceIds`, `tag:<TagKey>`,
///   `resource-groups:Name`, or `resource-groups:ResourceTypeFilters`.
///   `tag:` keys must have a non-empty `<TagKey>` suffix.
/// - `Values` set to a non-empty array of 1..=50 strings.
///
/// Returns an `InvalidParameters` error pointing at the first
/// violation; the rest of the list is not inspected.
pub(crate) fn validate_target_entry(entry: &Value) -> Result<(), AwsError> {
    let obj = entry.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameters",
            "Maintenance window target entry must be a JSON object.",
        )
    })?;
    let key = obj.get("Key").and_then(|v| v.as_str()).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameters",
            "Maintenance window target entry must have a string `Key`.",
        )
    })?;
    let key_ok = key == "InstanceIds"
        || key == "resource-groups:Name"
        || key == "resource-groups:ResourceTypeFilters"
        || key
            .strip_prefix("tag:")
            .map(|tail| !tail.is_empty())
            .unwrap_or(false);
    if !key_ok {
        return Err(AwsError::bad_request(
            "InvalidParameters",
            format!(
                "Maintenance window target Key `{key}` must be one of \
                 `InstanceIds`, `tag:<TagKey>`, `resource-groups:Name`, or \
                 `resource-groups:ResourceTypeFilters`."
            ),
        ));
    }
    let values = obj
        .get("Values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameters",
                format!("Maintenance window target `{key}` must have a `Values` array."),
            )
        })?;
    if values.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameters",
            format!("Maintenance window target `{key}` must have at least one value."),
        ));
    }
    if values.len() > MAX_TARGETS {
        return Err(AwsError::bad_request(
            "InvalidParameters",
            format!(
                "Maintenance window target `{key}` has {} values; the maximum is {MAX_TARGETS}.",
                values.len()
            ),
        ));
    }
    for v in values {
        if !v.is_string() {
            return Err(AwsError::bad_request(
                "InvalidParameters",
                format!("Maintenance window target `{key}` values must be strings."),
            ));
        }
    }
    Ok(())
}

/// Validate the `Targets` array on a maintenance-window register
/// call. Real AWS bounces oversize lists and malformed entries at
/// the API boundary, so we mirror that here.
pub(crate) fn validate_targets(targets: &[Value]) -> Result<(), AwsError> {
    if targets.len() > MAX_TARGETS {
        return Err(AwsError::bad_request(
            "InvalidParameters",
            format!(
                "Maintenance window `Targets` has {} entries; the maximum is {MAX_TARGETS}.",
                targets.len()
            ),
        ));
    }
    for entry in targets {
        validate_target_entry(entry)?;
    }
    Ok(())
}

/// Resolve a `Targets` list to the concrete set of instance IDs the
/// maintenance window would dispatch to. Only the `InstanceIds` key
/// resolves locally — `tag:*` and `resource-groups:*` resolutions
/// require an EC2 / ResourceGroups lookup that AWSim does not yet
/// thread into SSM, so those entries are skipped (returning the
/// empty set for now matches the behaviour of a maintenance window
/// whose targets reference instances that don't exist).
///
/// Used by [`describe_maintenance_windows_for_target`] to answer
/// "which windows would run against this instance?" without firing
/// the windows.
pub(crate) fn resolve_targets_to_instance_ids(targets: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in targets {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        if obj.get("Key").and_then(|v| v.as_str()) != Some("InstanceIds") {
            continue;
        }
        let Some(values) = obj.get("Values").and_then(|v| v.as_array()) else {
            continue;
        };
        for v in values {
            if let Some(id) = v.as_str() {
                let id = id.to_string();
                if seen.insert(id.clone()) {
                    out.push(id);
                }
            }
        }
    }
    out
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
    validate_targets(&targets)?;
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
    validate_targets(&targets)?;
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

/// Resolve a request-side `Targets` array (e.g. `Key=InstanceIds,
/// Values=i-abc`) to the maintenance windows that would dispatch
/// against any of those instances.
///
/// Real AWS evaluates this lazily — the SDK / console uses it to
/// preview "which windows would fire against my fleet?" without
/// actually firing them. AWSim mirrors that: every registered
/// target list is resolved through [`resolve_targets_to_instance_ids`],
/// and a window matches when at least one of its targets covers an
/// instance the caller asked about.
pub fn describe_maintenance_windows_for_target(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let requested = input["Targets"].as_array().cloned().unwrap_or_default();
    validate_targets(&requested)?;
    let requested_ids = resolve_targets_to_instance_ids(&requested);
    let requested_set: std::collections::HashSet<&str> =
        requested_ids.iter().map(String::as_str).collect();

    // Walk every registered window target and collect the windows
    // whose configured target set intersects the requested set.
    let mut matched: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for entry in state.maintenance_window_targets.iter() {
        let configured = resolve_targets_to_instance_ids(&entry.value().targets);
        let hit = configured
            .iter()
            .any(|id| requested_set.contains(id.as_str()));
        if hit {
            let window_id = entry.value().window_id.clone();
            if let Some(window) = state.maintenance_windows.get(&window_id) {
                matched.insert(window_id, window.name.clone());
            }
        }
    }

    let identities: Vec<Value> = matched
        .into_iter()
        .map(|(window_id, name)| json!({ "WindowId": window_id, "Name": name }))
        .collect();
    Ok(json!({ "WindowIdentities": identities }))
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

/// Deterministic mini-catalog of synthetic patches AWSim hands back
/// from Patch Manager queries. The shape mirrors what real Systems
/// Manager returns from `DescribeInstancePatches`: each entry has a
/// vendor-style `KBId`, a `Title`, a `Classification`
/// (SecurityUpdates / CriticalUpdates / ServicePacks / Updates), and
/// a `Severity` (Critical / Important / Moderate / Low).
///
/// Tests and SDK clients reading these don't need real CVE data —
/// they need predictable shapes so dashboards and compliance
/// reporting paths exercise correctly. The catalog is intentionally
/// small so the per-instance state (assigned via a deterministic
/// hash) is easy to reason about.
const SYNTHETIC_PATCHES: &[(&str, &str, &str, &str)] = &[
    (
        "KB5034441",
        "Security Update for Windows Server 2022 (KB5034441)",
        "SecurityUpdates",
        "Critical",
    ),
    (
        "KB5036561",
        "2024-04 Cumulative Update for Windows Server (KB5036561)",
        "CriticalUpdates",
        "Important",
    ),
    (
        "KB5037780",
        "Servicing Stack Update for Windows Server 2022 (KB5037780)",
        "ServicePacks",
        "Moderate",
    ),
    (
        "KB5040434",
        "Update for Microsoft Defender Antivirus (KB5040434)",
        "Updates",
        "Low",
    ),
    (
        "KB5042098",
        ".NET Framework Cumulative Update (KB5042098)",
        "SecurityUpdates",
        "Important",
    ),
];

/// Deterministic per-instance patch state: each instance gets a
/// stable mix of Installed / Missing / NotApplicable so subsequent
/// describe calls return the same answer, and so the InstancePatchStates
/// counts add up across `Installed + Missing + NotApplicable = total`.
fn synthetic_patch_state(instance_id: &str, kb_id: &str) -> &'static str {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    instance_id.hash(&mut h);
    kb_id.hash(&mut h);
    match h.finish() % 5 {
        // Bias toward "Installed" so reports look like a fleet that's
        // mostly patched — easier for tests to assert "InstalledCount
        // > 0" without needing exact counts.
        0 => "Missing",
        1 => "NotApplicable",
        _ => "Installed",
    }
}

/// Resolve a synthetic patch list for one instance. Returns
/// `(state, kb_id, title, classification, severity)` per patch so the
/// caller can shape it into either DescribeInstancePatches or the
/// per-state aggregates that DescribeInstancePatchStates surfaces.
fn instance_patch_findings(
    instance_id: &str,
) -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    SYNTHETIC_PATCHES
        .iter()
        .map(|(kb, title, class, sev)| {
            let state = synthetic_patch_state(instance_id, kb);
            (state, *kb, *title, *class, *sev)
        })
        .collect()
}

pub fn describe_instance_patches(
    _state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "InstanceId is required"))?;
    let findings = instance_patch_findings(instance_id);
    let patches: Vec<Value> = findings
        .into_iter()
        .map(|(state, kb, title, class, sev)| {
            json!({
                "KBId": kb,
                "Title": title,
                "Classification": class,
                "Severity": sev,
                "State": state,
                "InstalledTime": 0,
            })
        })
        .collect();
    Ok(json!({ "Patches": patches }))
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
            let findings = instance_patch_findings(&id);
            let mut installed = 0u32;
            let mut missing = 0u32;
            let mut not_applicable = 0u32;
            for (state, ..) in findings {
                match state {
                    "Installed" => installed += 1,
                    "Missing" => missing += 1,
                    "NotApplicable" => not_applicable += 1,
                    _ => {}
                }
            }
            json!({
                "InstanceId": id,
                "PatchGroup": "default",
                "BaselineId": "pb-default",
                "OperationStartTime": 0,
                "OperationEndTime": 0,
                "Operation": "Scan",
                "InstalledCount": installed,
                "MissingCount": missing,
                "FailedCount": 0,
                "NotApplicableCount": not_applicable,
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
    let patches: Vec<Value> = SYNTHETIC_PATCHES
        .iter()
        .map(|(kb, title, class, sev)| {
            json!({
                "Id": kb,
                "KbNumber": kb,
                "Title": title,
                "Classification": class,
                "Severity": sev,
            })
        })
        .collect();
    Ok(json!({ "Patches": patches }))
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

#[cfg(test)]
mod target_tests {
    use super::*;
    use crate::operations::documents::create_maintenance_window;

    fn ctx() -> RequestContext {
        RequestContext::new("ssm", "us-east-1")
    }

    fn make_window(state: &SsmState, name: &str) -> String {
        let resp = create_maintenance_window(
            state,
            &json!({
                "Name": name,
                "Schedule": "cron(0 0 * * ? *)",
                "Duration": 1,
                "Cutoff": 0,
            }),
            &ctx(),
        )
        .unwrap();
        resp["WindowId"].as_str().unwrap().to_string()
    }

    #[test]
    fn validate_target_rejects_non_object_entry() {
        let err = validate_target_entry(&json!("just-a-string")).unwrap_err();
        assert_eq!(err.code, "InvalidParameters");
    }

    #[test]
    fn validate_target_rejects_unknown_key() {
        let err = validate_target_entry(&json!({
            "Key": "nope:Name",
            "Values": ["v"]
        }))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameters");
        assert!(err.message.contains("Key"), "{:?}", err.message);
    }

    #[test]
    fn validate_target_accepts_documented_keys() {
        for key in [
            "InstanceIds",
            "tag:Environment",
            "resource-groups:Name",
            "resource-groups:ResourceTypeFilters",
        ] {
            validate_target_entry(&json!({
                "Key": key,
                "Values": ["v1"],
            }))
            .unwrap_or_else(|e| panic!("expected `{key}` to validate, got {e:?}"));
        }
    }

    #[test]
    fn validate_target_rejects_tag_key_without_suffix() {
        let err = validate_target_entry(&json!({
            "Key": "tag:",
            "Values": ["v"]
        }))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameters");
    }

    #[test]
    fn validate_target_requires_non_empty_values() {
        let err = validate_target_entry(&json!({
            "Key": "InstanceIds",
            "Values": []
        }))
        .unwrap_err();
        assert!(err.message.contains("at least one value"), "{err:?}");
    }

    #[test]
    fn register_target_rejects_malformed_target() {
        let state = SsmState::default();
        let window_id = make_window(&state, "win-1");
        let err = register_target_with_maintenance_window(
            &state,
            &json!({
                "WindowId": window_id,
                "Targets": [{ "Key": "InstanceIds", "Values": [] }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameters");
    }

    #[test]
    fn resolve_targets_dedups_instance_ids_across_entries() {
        let resolved = resolve_targets_to_instance_ids(&[
            json!({ "Key": "InstanceIds", "Values": ["i-a", "i-b"] }),
            json!({ "Key": "InstanceIds", "Values": ["i-b", "i-c"] }),
            json!({ "Key": "tag:Environment", "Values": ["prod"] }),
        ]);
        assert_eq!(resolved, vec!["i-a", "i-b", "i-c"]);
    }

    #[test]
    fn describe_windows_for_target_returns_only_intersecting_windows() {
        let state = SsmState::default();
        let win_a = make_window(&state, "win-a");
        let win_b = make_window(&state, "win-b");

        // win-a targets i-1, win-b targets i-2.
        register_target_with_maintenance_window(
            &state,
            &json!({
                "WindowId": win_a,
                "Targets": [{ "Key": "InstanceIds", "Values": ["i-1"] }],
            }),
            &ctx(),
        )
        .unwrap();
        register_target_with_maintenance_window(
            &state,
            &json!({
                "WindowId": win_b,
                "Targets": [{ "Key": "InstanceIds", "Values": ["i-2"] }],
            }),
            &ctx(),
        )
        .unwrap();

        let resp = describe_maintenance_windows_for_target(
            &state,
            &json!({
                "Targets": [{ "Key": "InstanceIds", "Values": ["i-1"] }],
            }),
            &ctx(),
        )
        .unwrap();
        let identities = resp["WindowIdentities"].as_array().unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0]["WindowId"], json!(win_a));
    }

    #[test]
    fn describe_windows_for_target_returns_empty_for_unmatched_instance() {
        let state = SsmState::default();
        let win = make_window(&state, "win-x");
        register_target_with_maintenance_window(
            &state,
            &json!({
                "WindowId": win,
                "Targets": [{ "Key": "InstanceIds", "Values": ["i-1"] }],
            }),
            &ctx(),
        )
        .unwrap();

        let resp = describe_maintenance_windows_for_target(
            &state,
            &json!({
                "Targets": [{ "Key": "InstanceIds", "Values": ["i-unknown"] }],
            }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["WindowIdentities"].as_array().unwrap().is_empty());
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("ssm", "us-east-1")
    }

    #[test]
    fn describe_instance_patches_returns_catalog_entries() {
        let state = SsmState::default();
        let resp = describe_instance_patches(
            &state,
            &json!({ "InstanceId": "i-aaaaaaaaaaaaaaaaa" }),
            &ctx(),
        )
        .unwrap();
        let patches = resp["Patches"].as_array().unwrap();
        assert_eq!(patches.len(), SYNTHETIC_PATCHES.len());
        // Every entry must carry the AWS-documented shape so downstream
        // SDK clients deserialise cleanly.
        for p in patches {
            for k in ["KBId", "Title", "Classification", "Severity", "State"] {
                assert!(p.get(k).is_some(), "missing {k} on {p}");
            }
        }
    }

    #[test]
    fn describe_instance_patches_is_deterministic_per_instance() {
        let state = SsmState::default();
        let a = describe_instance_patches(&state, &json!({ "InstanceId": "i-stable-1" }), &ctx())
            .unwrap();
        let b = describe_instance_patches(&state, &json!({ "InstanceId": "i-stable-1" }), &ctx())
            .unwrap();
        assert_eq!(a, b, "repeated scan must return the same finding set");
    }

    #[test]
    fn describe_instance_patch_states_counts_add_up() {
        let state = SsmState::default();
        let resp = describe_instance_patch_states(
            &state,
            &json!({ "InstanceIds": ["i-fleet-1", "i-fleet-2"] }),
            &ctx(),
        )
        .unwrap();
        let states = resp["InstancePatchStates"].as_array().unwrap();
        assert_eq!(states.len(), 2);
        for s in states {
            let installed = s["InstalledCount"].as_u64().unwrap();
            let missing = s["MissingCount"].as_u64().unwrap();
            let not_applicable = s["NotApplicableCount"].as_u64().unwrap();
            let failed = s["FailedCount"].as_u64().unwrap();
            assert_eq!(
                installed + missing + not_applicable + failed,
                SYNTHETIC_PATCHES.len() as u64,
                "per-instance counts must partition the synthetic catalog: {s}"
            );
        }
    }

    #[test]
    fn describe_available_patches_returns_catalog() {
        let state = SsmState::default();
        let resp = describe_available_patches(&state, &json!({}), &ctx()).unwrap();
        let patches = resp["Patches"].as_array().unwrap();
        assert_eq!(patches.len(), SYNTHETIC_PATCHES.len());
        assert!(patches.iter().all(|p| p["Id"].as_str().is_some()));
    }
}
