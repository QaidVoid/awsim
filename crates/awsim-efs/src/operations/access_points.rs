use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AccessPoint, EfsState};

fn new_ap_id() -> String {
    format!("fsap-{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

fn ap_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:elasticfilesystem:{}:{}:access-point/{}",
        ctx.region, ctx.account_id, id
    )
}

fn tags_to_array(tags: &HashMap<String, String>) -> Value {
    Value::Array(
        tags.iter()
            .map(|(k, v)| json!({ "Key": k, "Value": v }))
            .collect(),
    )
}

fn ap_to_value(ap: &AccessPoint) -> Value {
    json!({
        "AccessPointId": ap.access_point_id,
        "AccessPointArn": ap.access_point_arn,
        "ClientToken": ap.client_token,
        "FileSystemId": ap.file_system_id,
        "PosixUser": ap.posix_user,
        "RootDirectory": ap.root_directory,
        "LifeCycleState": ap.life_cycle_state,
        "Name": ap.name,
        "Tags": tags_to_array(&ap.tags),
        "OwnerId": "000000000000",
    })
}

pub fn create_access_point(
    state: &EfsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input
        .get("ClientToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "ClientToken is required"))?
        .to_string();
    let fs_id = input
        .get("FileSystemId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "FileSystemId is required"))?
        .to_string();

    if !state.file_systems.contains_key(&fs_id) {
        return Err(AwsError::not_found(
            "FileSystemNotFound",
            format!("File system {fs_id} not found"),
        ));
    }

    if let Some(existing) = state
        .access_points
        .iter()
        .find(|e| e.value().client_token == token)
    {
        return Ok(ap_to_value(existing.value()));
    }

    let tags: HashMap<String, String> = input
        .get("Tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    Some((
                        t.get("Key")?.as_str()?.to_string(),
                        t.get("Value")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let id = new_ap_id();
    let ap = AccessPoint {
        access_point_id: id.clone(),
        access_point_arn: ap_arn(ctx, &id),
        client_token: token,
        file_system_id: fs_id,
        posix_user: input.get("PosixUser").cloned(),
        root_directory: input.get("RootDirectory").cloned(),
        life_cycle_state: "available".to_string(),
        name: tags.get("Name").cloned(),
        tags,
    };
    let result = ap_to_value(&ap);
    state.access_points.insert(id, ap);
    Ok(result)
}

pub fn describe_access_points(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ap_filter = input.get("AccessPointId").and_then(|v| v.as_str());
    let fs_filter = input.get("FileSystemId").and_then(|v| v.as_str());

    let items: Vec<Value> = state
        .access_points
        .iter()
        .filter(|e| {
            if let Some(a) = ap_filter
                && e.value().access_point_id != a
            {
                return false;
            }
            if let Some(f) = fs_filter
                && e.value().file_system_id != f
            {
                return false;
            }
            true
        })
        .map(|e| ap_to_value(e.value()))
        .collect();
    Ok(json!({ "AccessPoints": items }))
}

pub fn delete_access_point(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("AccessPointId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "AccessPointId is required"))?;
    state.access_points.remove(id).ok_or_else(|| {
        AwsError::not_found(
            "AccessPointNotFound",
            format!("Access point {id} not found"),
        )
    })?;
    Ok(json!({}))
}
