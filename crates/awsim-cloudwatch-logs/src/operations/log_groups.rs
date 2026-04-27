use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::state::{LogGroup, LogsState};

// ---------------------------------------------------------------------------
// CreateLogGroup
// ---------------------------------------------------------------------------

pub fn create_log_group(
    state: &LogsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    if state.log_groups.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Log group already exists: {name}"),
        ));
    }

    let arn = format!(
        "arn:aws:logs:{}:{}:log-group:{}",
        ctx.region, ctx.account_id, name
    );

    let mut tags: HashMap<String, String> = HashMap::new();
    if let Some(tag_map) = input["tags"].as_object() {
        for (k, v) in tag_map {
            if let Some(s) = v.as_str() {
                tags.insert(k.clone(), s.to_string());
            }
        }
    }

    let group = LogGroup::new(name.to_string(), arn.clone(), tags);
    info!(log_group = %name, "Created log group");
    state.log_groups.insert(name.to_string(), group);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteLogGroup
// ---------------------------------------------------------------------------

pub fn delete_log_group(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    state.log_groups.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    if let Some(bs) = state.body_store()
        && let Err(e) = bs.delete_bucket("cloudwatch-logs", name)
    {
        warn!(
            log_group = %name,
            error = %e,
            "Failed to remove persisted log group directory"
        );
    }

    info!(log_group = %name, "Deleted log group");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeLogGroups
// ---------------------------------------------------------------------------

pub fn describe_log_groups(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let prefix = input["logGroupNamePrefix"].as_str().unwrap_or("");
    let limit = input["limit"].as_u64().unwrap_or(50).min(50) as usize;
    let next_token = input["nextToken"].as_str().unwrap_or("");

    let mut groups: Vec<Value> = state
        .log_groups
        .iter()
        .filter(|e| e.key().starts_with(prefix))
        .map(|e| {
            let g = e.value();
            let mut obj = json!({
                "logGroupName": g.name,
                "arn": g.arn,
                "creationTime": g.creation_time,
                "storedBytes": g.stored_bytes,
                "metricFilterCount": 0,
            });
            if let Some(days) = g.retention_in_days {
                obj["retentionInDays"] = json!(days);
            }
            obj
        })
        .collect();

    // Sort by name for stable pagination
    groups.sort_by(|a, b| {
        a["logGroupName"]
            .as_str()
            .unwrap_or("")
            .cmp(b["logGroupName"].as_str().unwrap_or(""))
    });

    // Apply nextToken offset
    let start = if next_token.is_empty() {
        0
    } else {
        groups
            .iter()
            .position(|g| g["logGroupName"].as_str().unwrap_or("") > next_token)
            .unwrap_or(groups.len())
    };

    let page = &groups[start..];
    let page: Vec<Value> = page.iter().take(limit).cloned().collect();
    let new_next_token = if start + limit < groups.len() {
        page.last()
            .and_then(|g| g["logGroupName"].as_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    let mut result = json!({ "logGroups": page });
    if let Some(token) = new_next_token {
        result["nextToken"] = json!(token);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// PutRetentionPolicy
// ---------------------------------------------------------------------------

pub fn put_retention_policy(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let days = input["retentionInDays"].as_u64().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "retentionInDays is required")
    })? as u32;

    let valid_days = [
        1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557,
        2922, 3288, 3653,
    ];
    if !valid_days.contains(&days) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "retentionInDays must be one of the valid values",
        ));
    }

    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    group.retention_in_days = Some(days);
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteRetentionPolicy
// ---------------------------------------------------------------------------

pub fn delete_retention_policy(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    group.retention_in_days = None;
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// TagLogGroup
// ---------------------------------------------------------------------------

pub fn tag_log_group(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let tags = input["tags"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "tags is required"))?;

    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    for (k, v) in tags {
        if let Some(s) = v.as_str() {
            group.tags.insert(k.clone(), s.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagLogGroup
// ---------------------------------------------------------------------------

pub fn untag_log_group(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let keys = input["tags"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "tags (key list) is required")
    })?;

    let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    for key in keys {
        if let Some(k) = key.as_str() {
            group.tags.remove(k);
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTagsLogGroup
// ---------------------------------------------------------------------------

pub fn list_tags_log_group(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    let group = state.log_groups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    let tags: serde_json::Map<String, Value> = group
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "tags": tags }))
}
