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

    let log_group_class = input["logGroupClass"].as_str().unwrap_or("STANDARD");
    if !matches!(log_group_class, "STANDARD" | "INFREQUENT_ACCESS") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("logGroupClass `{log_group_class}` must be STANDARD or INFREQUENT_ACCESS."),
        ));
    }

    // kmsKeyId is optional. AWS validates the ARN prefix and rejects
    // anything that isn't a KMS key reference up front.
    let kms_key_id = match input["kmsKeyId"].as_str() {
        Some(s) if !s.is_empty() => {
            if !s.starts_with("arn:aws:kms:") {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!("kmsKeyId `{s}` must be a KMS key ARN."),
                ));
            }
            Some(s.to_string())
        }
        _ => None,
    };

    // logGroupClass-style deletion-protection toggle: AWS lets callers
    // set this at create time (and via PutDataProtectionPolicy) to
    // refuse subsequent DeleteLogGroup calls.
    let deletion_protection = input["logGroupDeletionProtection"]
        .as_str()
        .unwrap_or("DISABLED")
        .to_string();
    if !matches!(deletion_protection.as_str(), "DISABLED" | "ENABLED") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "logGroupDeletionProtection `{deletion_protection}` must be ENABLED or DISABLED."
            ),
        ));
    }

    let mut group = LogGroup::new(name.to_string(), arn.clone(), tags);
    group.log_group_class = log_group_class.to_string();
    group.kms_key_id = kms_key_id;
    group.deletion_protection = deletion_protection;
    info!(log_group = %name, class = %log_group_class, "Created log group");
    state.log_groups.insert(name.to_string(), group);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteLogGroup
// ---------------------------------------------------------------------------

pub fn delete_log_group(
    state: &LogsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["logGroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "logGroupName is required")
    })?;

    {
        let group = state.log_groups.get(name).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Log group not found: {name}"),
            )
        })?;
        if group.deletion_protection == "ENABLED" {
            return Err(AwsError::conflict(
                "OperationAbortedException",
                format!(
                    "Log group `{name}` has deletion protection enabled; turn it off before deleting."
                ),
            ));
        }
    }
    state.log_groups.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Log group not found: {name}"),
        )
    })?;

    if let Some(sqlite) = state.sqlite()
        && let Err(e) = sqlite.delete_group(&ctx.account_id, &ctx.region, name)
    {
        warn!(
            log_group = %name,
            error = %e.message,
            "Failed to remove persisted log group events"
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
                "logGroupClass": g.log_group_class,
                "logGroupDeletionProtection": g.deletion_protection,
            });
            if let Some(days) = g.retention_in_days {
                obj["retentionInDays"] = json!(days);
            }
            if let Some(ref k) = g.kms_key_id {
                obj["kmsKeyId"] = json!(k);
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
// AssociateKmsKey / DisassociateKmsKey
// ---------------------------------------------------------------------------

/// True when the resource identifier targets the account-level
/// "query result" scope (`<accountId>:query-result` or the literal
/// `query-result`). AWS distinguishes log-group encryption from
/// query-result encryption via this identifier.
fn is_query_result_scope(resource_identifier: &str) -> bool {
    resource_identifier == "query-result"
        || resource_identifier
            .rsplit_once(':')
            .map(|(_, tail)| tail == "query-result")
            .unwrap_or(false)
}

/// `AssociateKmsKey`. AWS routes the assignment by the optional
/// `resourceIdentifier`: when it names a query-result scope, the key
/// encrypts query results at the account level; otherwise it pairs
/// with the named log group. Callers must pass at least one target.
pub fn associate_kms_key(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let kms_key_id = input["kmsKeyId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "kmsKeyId is required")
    })?;
    if !kms_key_id.contains("arn:aws:kms:") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "kmsKeyId must be a KMS key ARN.",
        ));
    }

    let resource_identifier = input["resourceIdentifier"].as_str();
    let log_group_name = input["logGroupName"].as_str();

    match (resource_identifier, log_group_name) {
        (Some(rid), _) if is_query_result_scope(rid) => {
            *state.query_result_kms_key_id.lock().unwrap() = Some(kms_key_id.to_string());
            Ok(json!({}))
        }
        (_, Some(name)) => {
            let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Log group not found: {name}"),
                )
            })?;
            group.kms_key_id = Some(kms_key_id.to_string());
            Ok(json!({}))
        }
        _ => Err(AwsError::bad_request(
            "InvalidParameterException",
            "Either logGroupName or a query-result resourceIdentifier is required.",
        )),
    }
}

/// `DisassociateKmsKey`. Same routing rules as `AssociateKmsKey`: a
/// query-result `resourceIdentifier` clears the account-level key;
/// otherwise the named log group's `kmsKeyId` is cleared. No-op
/// against a log group with no key set (matches AWS).
pub fn disassociate_kms_key(
    state: &LogsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_identifier = input["resourceIdentifier"].as_str();
    let log_group_name = input["logGroupName"].as_str();

    match (resource_identifier, log_group_name) {
        (Some(rid), _) if is_query_result_scope(rid) => {
            *state.query_result_kms_key_id.lock().unwrap() = None;
            Ok(json!({}))
        }
        (_, Some(name)) => {
            let mut group = state.log_groups.get_mut(name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Log group not found: {name}"),
                )
            })?;
            group.kms_key_id = None;
            Ok(json!({}))
        }
        _ => Err(AwsError::bad_request(
            "InvalidParameterException",
            "Either logGroupName or a query-result resourceIdentifier is required.",
        )),
    }
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

#[cfg(test)]
mod deletion_protection_tests {
    use super::*;
    use crate::SqliteStore;
    use std::sync::Arc;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn fresh_state() -> LogsState {
        let dir = std::env::temp_dir().join(format!("awsim-logs-dp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(SqliteStore::open(dir.join("logs.db")).unwrap());
        let state = LogsState::default();
        state.set_sqlite(store);
        state
    }

    #[test]
    fn create_log_group_persists_kms_key_and_deletion_protection() {
        let state = fresh_state();
        create_log_group(
            &state,
            &json!({
                "logGroupName": "g",
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abcd",
                "logGroupDeletionProtection": "ENABLED",
            }),
            &ctx(),
        )
        .unwrap();
        let g = state.log_groups.get("g").unwrap();
        assert_eq!(g.deletion_protection, "ENABLED");
        assert_eq!(
            g.kms_key_id.as_deref(),
            Some("arn:aws:kms:us-east-1:000000000000:key/abcd")
        );
    }

    #[test]
    fn create_log_group_rejects_non_kms_key_id() {
        let state = fresh_state();
        let err = create_log_group(
            &state,
            &json!({
                "logGroupName": "g",
                "kmsKeyId": "not-an-arn",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_log_group_rejects_invalid_deletion_protection_value() {
        let state = fresh_state();
        let err = create_log_group(
            &state,
            &json!({
                "logGroupName": "g",
                "logGroupDeletionProtection": "MAYBE",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn delete_log_group_refuses_when_protection_enabled() {
        let state = fresh_state();
        create_log_group(
            &state,
            &json!({
                "logGroupName": "g",
                "logGroupDeletionProtection": "ENABLED",
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "OperationAbortedException");
    }

    #[test]
    fn delete_log_group_succeeds_when_protection_disabled() {
        let state = fresh_state();
        create_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        delete_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        assert!(state.log_groups.get("g").is_none());
    }

    #[test]
    fn associate_kms_key_targets_log_group_when_no_query_result_id() {
        let state = fresh_state();
        create_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        associate_kms_key(
            &state,
            &json!({
                "logGroupName": "g",
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap();
        let group = state.log_groups.get("g").unwrap();
        assert_eq!(
            group.kms_key_id.as_deref(),
            Some("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
        // Account-level query-result key untouched.
        assert!(state.query_result_kms_key_id.lock().unwrap().is_none());
    }

    #[test]
    fn associate_kms_key_targets_query_result_via_resource_identifier() {
        let state = fresh_state();
        associate_kms_key(
            &state,
            &json!({
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/qr",
                "resourceIdentifier": "000000000000:query-result",
            }),
            &ctx(),
        )
        .unwrap();
        // Account-level key set.
        assert_eq!(
            state.query_result_kms_key_id.lock().unwrap().as_deref(),
            Some("arn:aws:kms:us-east-1:000000000000:key/qr")
        );
        // No log group exists; the call must succeed without one
        // because the resourceIdentifier routed past the log-group
        // path.
    }

    #[test]
    fn disassociate_kms_key_clears_log_group_only() {
        let state = fresh_state();
        create_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        associate_kms_key(
            &state,
            &json!({
                "logGroupName": "g",
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap();
        associate_kms_key(
            &state,
            &json!({
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/qr",
                "resourceIdentifier": "query-result",
            }),
            &ctx(),
        )
        .unwrap();
        // Disassociating the log group must not clear the query-result key.
        disassociate_kms_key(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        assert!(state.log_groups.get("g").unwrap().kms_key_id.is_none());
        assert!(state.query_result_kms_key_id.lock().unwrap().is_some());
    }

    #[test]
    fn associate_kms_key_requires_at_least_one_target() {
        let state = fresh_state();
        let err = associate_kms_key(
            &state,
            &json!({
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn associate_kms_key_rejects_non_kms_arn() {
        let state = fresh_state();
        create_log_group(&state, &json!({ "logGroupName": "g" }), &ctx()).unwrap();
        let err = associate_kms_key(
            &state,
            &json!({
                "logGroupName": "g",
                "kmsKeyId": "not-an-arn",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }
}
