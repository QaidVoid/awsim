use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{EfsState, MountTarget};

fn new_mt_id() -> String {
    format!("fsmt-{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

fn new_eni_id() -> String {
    format!("eni-{}", &uuid::Uuid::new_v4().simple().to_string()[..17])
}

fn mt_to_value(mt: &MountTarget) -> Value {
    json!({
        "MountTargetId": mt.mount_target_id,
        "FileSystemId": mt.file_system_id,
        "SubnetId": mt.subnet_id,
        "LifeCycleState": mt.life_cycle_state,
        "IpAddress": mt.ip_address,
        "NetworkInterfaceId": mt.network_interface_id,
        "AvailabilityZoneId": mt.availability_zone_id,
        "AvailabilityZoneName": mt.availability_zone_name,
        "VpcId": mt.vpc_id,
        "OwnerId": "000000000000",
    })
}

pub fn create_mount_target(
    state: &EfsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let fs_id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?
        .to_string();
    let subnet_id = input
        .get("SubnetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "SubnetId is required"))?
        .to_string();

    let fs_az_pinned = match state.file_systems.get(&fs_id) {
        None => {
            return Err(AwsError::not_found(
                "FileSystemNotFound",
                format!("File system {fs_id} not found"),
            ));
        }
        Some(fs) => fs.availability_zone_name.is_some() || fs.availability_zone_id.is_some(),
    };
    if fs_az_pinned
        && state
            .mount_targets
            .iter()
            .any(|e| e.value().file_system_id == fs_id)
    {
        return Err(AwsError::bad_request(
            "MountTargetConflict",
            "One Zone file systems support only a single mount target.",
        ));
    }
    // AWS rejects a second mount target in the same subnet for the
    // same file system with MountTargetConflict.
    if state.mount_targets.iter().any(|e| {
        let v = e.value();
        v.file_system_id == fs_id && v.subnet_id == subnet_id
    }) {
        return Err(AwsError::bad_request(
            "MountTargetConflict",
            format!("A mount target already exists for file system {fs_id} in subnet {subnet_id}.",),
        ));
    }

    // AWS caps mount-target security groups at 5; SecurityGroupLimitExceeded
    // is the documented error code.
    let security_groups: Vec<String> = input
        .get("SecurityGroups")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| vec!["sg-default".to_string()]);
    if security_groups.len() > 5 {
        return Err(AwsError::bad_request(
            "SecurityGroupLimitExceeded",
            format!(
                "A mount target may attach at most 5 security groups (got {}).",
                security_groups.len()
            ),
        ));
    }

    let id = new_mt_id();
    let mt = MountTarget {
        mount_target_id: id.clone(),
        file_system_id: fs_id.clone(),
        subnet_id,
        life_cycle_state: "available".to_string(),
        ip_address: input
            .get("IpAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("10.0.0.10")
            .to_string(),
        network_interface_id: new_eni_id(),
        availability_zone_id: format!("{}-az1", ctx.region),
        availability_zone_name: format!("{}a", ctx.region),
        vpc_id: "vpc-default".to_string(),
        security_groups,
    };
    let result = mt_to_value(&mt);
    state.mount_targets.insert(id, mt);

    if let Some(mut fs) = state.file_systems.get_mut(&fs_id) {
        fs.number_of_mount_targets += 1;
    }
    Ok(result)
}

pub fn describe_mount_targets(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let fs_filter = input.get("FileSystemId").and_then(|v| v.as_str());
    let mt_filter = input.get("MountTargetId").and_then(|v| v.as_str());
    let max_items = awsim_core::clamp_max_results_strict(
        input.get("MaxItems").and_then(Value::as_i64),
        100,
        1000,
    )?;
    let marker = input.get("Marker").and_then(Value::as_str);
    let mut entries: Vec<(String, Value)> = state
        .mount_targets
        .iter()
        .filter(|e| {
            if let Some(f) = fs_filter
                && e.value().file_system_id != f
            {
                return false;
            }
            if let Some(m) = mt_filter
                && e.value().mount_target_id != m
            {
                return false;
            }
            true
        })
        .map(|e| (e.value().mount_target_id.clone(), mt_to_value(e.value())))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let page = awsim_core::paginate(entries, max_items, marker, |(k, _)| k.clone())?;
    let items: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();
    let mut body = json!({ "MountTargets": items });
    if let Some(token) = page.next_token {
        body["NextMarker"] = json!(token);
    }
    Ok(body)
}

pub fn delete_mount_target(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("MountTargetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "MountTargetId is required"))?;
    let (_, mt) = state.mount_targets.remove(id).ok_or_else(|| {
        AwsError::not_found(
            "MountTargetNotFound",
            format!("Mount target {id} not found"),
        )
    })?;
    if let Some(mut fs) = state.file_systems.get_mut(&mt.file_system_id)
        && fs.number_of_mount_targets > 0
    {
        fs.number_of_mount_targets -= 1;
    }
    Ok(json!({}))
}

pub fn describe_mount_target_security_groups(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("MountTargetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "MountTargetId is required"))?;
    let mt = state.mount_targets.get(id).ok_or_else(|| {
        AwsError::not_found(
            "MountTargetNotFound",
            format!("Mount target {id} not found"),
        )
    })?;
    Ok(json!({ "SecurityGroups": mt.security_groups }))
}

pub fn modify_mount_target_security_groups(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("MountTargetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "MountTargetId is required"))?;
    let groups = input
        .get("SecurityGroups")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut mt = state.mount_targets.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "MountTargetNotFound",
            format!("Mount target {id} not found"),
        )
    })?;
    mt.security_groups = groups;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::file_systems::create_file_system;
    use crate::state::EfsState;

    fn ctx() -> RequestContext {
        RequestContext::new("efs", "us-east-1")
    }

    #[test]
    fn create_mount_target_rejects_duplicate_subnet() {
        let state = EfsState::default();
        let resp =
            create_file_system(&state, &json!({ "CreationToken": "t-dup" }), &ctx()).unwrap();
        let fs_id = resp["FileSystemId"].as_str().unwrap().to_string();
        create_mount_target(
            &state,
            &json!({ "FileSystemId": fs_id, "SubnetId": "subnet-a" }),
            &ctx(),
        )
        .unwrap();
        let err = create_mount_target(
            &state,
            &json!({ "FileSystemId": fs_id, "SubnetId": "subnet-a" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MountTargetConflict");
    }

    #[test]
    fn create_mount_target_caps_security_groups_at_five() {
        let state = EfsState::default();
        let resp = create_file_system(&state, &json!({ "CreationToken": "t-sg" }), &ctx()).unwrap();
        let fs_id = resp["FileSystemId"].as_str().unwrap().to_string();
        let err = create_mount_target(
            &state,
            &json!({
                "FileSystemId": fs_id,
                "SubnetId": "subnet-cap",
                "SecurityGroups": [
                    "sg-1", "sg-2", "sg-3", "sg-4", "sg-5", "sg-6",
                ],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "SecurityGroupLimitExceeded");
    }

    #[test]
    fn one_zone_file_system_rejects_second_mount_target() {
        let state = EfsState::default();
        let resp = create_file_system(
            &state,
            &json!({
                "CreationToken": "t-az-onezone",
                "AvailabilityZoneName": "us-east-1a",
            }),
            &ctx(),
        )
        .unwrap();
        let fs_id = resp["FileSystemId"].as_str().unwrap().to_string();
        create_mount_target(
            &state,
            &json!({ "FileSystemId": fs_id, "SubnetId": "subnet-a" }),
            &ctx(),
        )
        .unwrap();
        let err = create_mount_target(
            &state,
            &json!({ "FileSystemId": fs_id, "SubnetId": "subnet-b" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MountTargetConflict");
    }
}
