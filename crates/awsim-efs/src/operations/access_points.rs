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

/// Validates `PosixUser` fields when supplied. AWS requires Uid and
/// Gid as non-negative integers and caps `SecondaryGids` at 16.
fn validate_posix_user(posix: Option<&Value>) -> Result<(), AwsError> {
    let Some(p) = posix else { return Ok(()) };
    let uid = p.get("Uid").and_then(Value::as_i64).ok_or_else(|| {
        AwsError::bad_request(
            "BadRequest",
            "PosixUser.Uid is required and must be an integer",
        )
    })?;
    let gid = p.get("Gid").and_then(Value::as_i64).ok_or_else(|| {
        AwsError::bad_request(
            "BadRequest",
            "PosixUser.Gid is required and must be an integer",
        )
    })?;
    if uid < 0 || gid < 0 {
        return Err(AwsError::bad_request(
            "BadRequest",
            "PosixUser.Uid and Gid must be non-negative.",
        ));
    }
    if let Some(secondary) = p.get("SecondaryGids").and_then(Value::as_array) {
        if secondary.len() > 16 {
            return Err(AwsError::bad_request(
                "BadRequest",
                format!(
                    "PosixUser.SecondaryGids may have at most 16 entries (got {}).",
                    secondary.len()
                ),
            ));
        }
        for v in secondary {
            let g = v.as_i64().ok_or_else(|| {
                AwsError::bad_request(
                    "BadRequest",
                    "PosixUser.SecondaryGids entries must be integers.",
                )
            })?;
            if g < 0 {
                return Err(AwsError::bad_request(
                    "BadRequest",
                    "PosixUser.SecondaryGids entries must be non-negative.",
                ));
            }
        }
    }
    Ok(())
}

/// Validates `RootDirectory`. AWS requires `CreationInfo` (with
/// OwnerUid / OwnerGid / Permissions) whenever Path is not `/`, and
/// Permissions must parse as a 1-4 digit octal string.
fn validate_root_directory(root: Option<&Value>) -> Result<(), AwsError> {
    let Some(rd) = root else { return Ok(()) };
    let path = rd
        .get("Path")
        .and_then(Value::as_str)
        .unwrap_or("/")
        .to_string();
    let creation_info = rd.get("CreationInfo");
    if path != "/" && creation_info.is_none() {
        return Err(AwsError::bad_request(
            "BadRequest",
            "RootDirectory.CreationInfo is required when Path != \"/\".",
        ));
    }
    if let Some(ci) = creation_info {
        ci.get("OwnerUid").and_then(Value::as_i64).ok_or_else(|| {
            AwsError::bad_request("BadRequest", "CreationInfo.OwnerUid is required.")
        })?;
        ci.get("OwnerGid").and_then(Value::as_i64).ok_or_else(|| {
            AwsError::bad_request("BadRequest", "CreationInfo.OwnerGid is required.")
        })?;
        let perms = ci
            .get("Permissions")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AwsError::bad_request(
                    "BadRequest",
                    "CreationInfo.Permissions is required (octal string).",
                )
            })?;
        if perms.is_empty() || perms.len() > 4 || !perms.chars().all(|c| ('0'..='7').contains(&c)) {
            return Err(AwsError::bad_request(
                "BadRequest",
                format!("CreationInfo.Permissions `{perms}` must be a 1-4 digit octal."),
            ));
        }
    }
    Ok(())
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

    // AWS keeps a 24h idempotency cache keyed by ClientToken; a replay
    // with the same parameters returns the cached body, while a replay
    // with different parameters raises IdempotencyParameterMismatch.
    let request_hash = awsim_core::idempotency::hash_request(&format!(
        "create_access_point:{fs_id}:{posix}:{root}",
        posix = input
            .get("PosixUser")
            .map(|v| v.to_string())
            .unwrap_or_default(),
        root = input
            .get("RootDirectory")
            .map(|v| v.to_string())
            .unwrap_or_default(),
    ));
    match state.access_point_idempotency.lookup(&token, request_hash) {
        awsim_core::idempotency::Lookup::Hit(v) => return Ok(v),
        awsim_core::idempotency::Lookup::Mismatch => {
            return Err(AwsError::bad_request(
                "IdempotencyParameterMismatchException",
                format!("ClientToken `{token}` was already used with different arguments.",),
            ));
        }
        awsim_core::idempotency::Lookup::Miss => {}
    }

    validate_posix_user(input.get("PosixUser"))?;
    validate_root_directory(input.get("RootDirectory"))?;

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
        client_token: token.clone(),
        file_system_id: fs_id,
        posix_user: input.get("PosixUser").cloned(),
        root_directory: input.get("RootDirectory").cloned(),
        life_cycle_state: "available".to_string(),
        name: tags.get("Name").cloned(),
        tags,
    };
    let result = ap_to_value(&ap);
    state
        .access_point_idempotency
        .insert(&token, request_hash, result.clone());
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
    let max_items = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        100,
        1000,
    )?;
    let next_token = input.get("NextToken").and_then(Value::as_str);
    let mut entries: Vec<(String, Value)> = state
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
        .map(|e| (e.value().access_point_id.clone(), ap_to_value(e.value())))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let page = awsim_core::paginate(entries, max_items, next_token, |(k, _)| k.clone())?;
    let items: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();
    let mut body = json!({ "AccessPoints": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::file_systems::create_file_system;
    use crate::state::EfsState;

    fn ctx() -> RequestContext {
        RequestContext::new("efs", "us-east-1")
    }

    fn fs_id(state: &EfsState) -> String {
        let resp = create_file_system(state, &json!({ "CreationToken": "t-ap" }), &ctx()).unwrap();
        resp["FileSystemId"].as_str().unwrap().to_string()
    }

    #[test]
    fn create_access_point_replays_cached_response_on_same_token() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        let body = json!({
            "ClientToken": "t-idem",
            "FileSystemId": fs,
            "PosixUser": { "Uid": 1000, "Gid": 1000 },
        });
        let first = create_access_point(&state, &body, &ctx()).unwrap();
        let second = create_access_point(&state, &body, &ctx()).unwrap();
        assert_eq!(first["AccessPointId"], second["AccessPointId"]);
    }

    #[test]
    fn create_access_point_rejects_param_mismatch_on_replay() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        create_access_point(
            &state,
            &json!({
                "ClientToken": "t-mismatch",
                "FileSystemId": fs,
                "PosixUser": { "Uid": 1, "Gid": 1 },
            }),
            &ctx(),
        )
        .unwrap();
        let err = create_access_point(
            &state,
            &json!({
                "ClientToken": "t-mismatch",
                "FileSystemId": fs,
                "PosixUser": { "Uid": 2, "Gid": 2 },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "IdempotencyParameterMismatchException");
    }

    #[test]
    fn create_access_point_rejects_invalid_permissions() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        let err = create_access_point(
            &state,
            &json!({
                "ClientToken": "t-perms",
                "FileSystemId": fs,
                "RootDirectory": {
                    "Path": "/app",
                    "CreationInfo": {
                        "OwnerUid": 1000,
                        "OwnerGid": 1000,
                        "Permissions": "9999",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_access_point_requires_creation_info_when_path_not_root() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        let err = create_access_point(
            &state,
            &json!({
                "ClientToken": "t-missing-ci",
                "FileSystemId": fs,
                "RootDirectory": { "Path": "/team-x" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_access_point_caps_secondary_gids() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        let secondary: Vec<u32> = (0..17).collect();
        let err = create_access_point(
            &state,
            &json!({
                "ClientToken": "t-gids",
                "FileSystemId": fs,
                "PosixUser": { "Uid": 1, "Gid": 2, "SecondaryGids": secondary },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequest");
    }

    #[test]
    fn create_access_point_accepts_valid_octal_perms() {
        let state = EfsState::default();
        let fs = fs_id(&state);
        create_access_point(
            &state,
            &json!({
                "ClientToken": "t-ok",
                "FileSystemId": fs,
                "PosixUser": { "Uid": 1000, "Gid": 1000 },
                "RootDirectory": {
                    "Path": "/data",
                    "CreationInfo": {
                        "OwnerUid": 1000,
                        "OwnerGid": 1000,
                        "Permissions": "0755",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap();
    }
}
