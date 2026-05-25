use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EfsState;

/// EFS tags live on file systems and access points; resource is identified by id.
fn split_resource(id: &str) -> (&str, &str) {
    if id.starts_with("fs-") {
        ("fs", id)
    } else if id.starts_with("fsap-") {
        ("ap", id)
    } else {
        ("unknown", id)
    }
}

pub fn tag_resource(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ResourceId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "ResourceId is required"))?;
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;

    let tags = input
        .get("Tags")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "Tags is required"))?;

    let (kind, key) = split_resource(id);
    match kind {
        "fs" => {
            if let Some(mut fs) = state.file_systems.get_mut(key) {
                for t in tags {
                    if let (Some(k), Some(v)) = (
                        t.get("Key").and_then(|v| v.as_str()),
                        t.get("Value").and_then(|v| v.as_str()),
                    ) {
                        fs.tags.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
        "ap" => {
            if let Some(mut ap) = state.access_points.get_mut(key) {
                for t in tags {
                    if let (Some(k), Some(v)) = (
                        t.get("Key").and_then(|v| v.as_str()),
                        t.get("Value").and_then(|v| v.as_str()),
                    ) {
                        ap.tags.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ResourceId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "ResourceId is required"))?;
    validate_aws_tag_keys(&input["TagKeys"])?;

    let keys = input
        .get("TagKeys")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "TagKeys is required"))?;

    let (kind, key) = split_resource(id);
    match kind {
        "fs" => {
            if let Some(mut fs) = state.file_systems.get_mut(key) {
                for k in keys {
                    if let Some(s) = k.as_str() {
                        fs.tags.remove(s);
                    }
                }
            }
        }
        "ap" => {
            if let Some(mut ap) = state.access_points.get_mut(key) {
                for k in keys {
                    if let Some(s) = k.as_str() {
                        ap.tags.remove(s);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &EfsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ResourceId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequest", "ResourceId is required"))?;
    let (kind, key) = split_resource(id);
    let tags = match kind {
        "fs" => state.file_systems.get(key).map(|fs| fs.tags.clone()),
        "ap" => state.access_points.get(key).map(|ap| ap.tags.clone()),
        _ => None,
    }
    .unwrap_or_default();
    let arr: Vec<Value> = tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();
    Ok(json!({ "Tags": arr }))
}
