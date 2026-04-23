use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Archive, EventBridgeState};

fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn archive_to_value(a: &Archive) -> Value {
    json!({
        "ArchiveName": a.name,
        "ArchiveArn": a.arn,
        "EventSourceArn": a.event_source_arn,
        "Description": a.description,
        "EventPattern": a.event_pattern,
        "RetentionDays": a.retention_days,
        "State": a.state,
        "CreationTime": a.creation_time,
    })
}

// ---------------------------------------------------------------------------
// CreateArchive
// ---------------------------------------------------------------------------

pub fn create_archive(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ArchiveName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ArchiveName is required"))?;

    if state.archives.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Archive {name} already exists"),
        ));
    }

    let event_source_arn = input["EventSourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EventSourceArn is required"))?;

    let arn = format!(
        "arn:aws:events:{}:{}:archive/{}",
        ctx.region, ctx.account_id, name
    );

    let archive = Archive {
        name: name.to_string(),
        arn: arn.clone(),
        event_source_arn: event_source_arn.to_string(),
        description: input["Description"].as_str().unwrap_or("").to_string(),
        event_pattern: input["EventPattern"].as_str().map(|s| s.to_string()),
        retention_days: input["RetentionDays"].as_u64().unwrap_or(0) as u32,
        state: "ENABLED".to_string(),
        creation_time: now_iso8601(),
    };

    state.archives.insert(name.to_string(), archive);

    Ok(json!({
        "ArchiveArn": arn,
        "State": "ENABLED",
        "CreationTime": state.archives.get(name).map(|a| a.creation_time.clone()).unwrap_or_default(),
    }))
}

// ---------------------------------------------------------------------------
// DeleteArchive
// ---------------------------------------------------------------------------

pub fn delete_archive(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ArchiveName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ArchiveName is required"))?;

    state.archives.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Archive {name} does not exist"),
        )
    })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeArchive
// ---------------------------------------------------------------------------

pub fn describe_archive(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ArchiveName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ArchiveName is required"))?;

    let archive = state.archives.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Archive {name} does not exist"),
        )
    })?;

    Ok(archive_to_value(&archive))
}

// ---------------------------------------------------------------------------
// ListArchives
// ---------------------------------------------------------------------------

pub fn list_archives(
    state: &EventBridgeState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let archives: Vec<Value> = state
        .archives
        .iter()
        .map(|entry| archive_to_value(entry.value()))
        .collect();

    Ok(json!({ "Archives": archives }))
}
