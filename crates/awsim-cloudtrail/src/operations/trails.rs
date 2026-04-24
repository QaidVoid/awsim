use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudTrailState, Trail, TrailStatus, now_secs};

pub fn create_trail(
    state: &CloudTrailState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let bucket = input["S3BucketName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "S3BucketName is required"))?;
    let arn = format!(
        "arn:aws:cloudtrail:{}:{}:trail/{}",
        ctx.region, ctx.account_id, name
    );

    let trail = Trail {
        name: name.to_string(),
        arn: arn.clone(),
        s3_bucket_name: bucket.to_string(),
        s3_key_prefix: input["S3KeyPrefix"].as_str().map(String::from),
        sns_topic_name: input["SnsTopicName"].as_str().map(String::from),
        sns_topic_arn: input["SnsTopicName"]
            .as_str()
            .map(|t| format!("arn:aws:sns:{}:{}:{}", ctx.region, ctx.account_id, t)),
        include_global_service_events: input["IncludeGlobalServiceEvents"]
            .as_bool()
            .unwrap_or(true),
        is_multi_region_trail: input["IsMultiRegionTrail"].as_bool().unwrap_or(false),
        home_region: ctx.region.clone(),
        log_file_validation_enabled: input["EnableLogFileValidation"].as_bool().unwrap_or(false),
        cloud_watch_logs_log_group_arn: input["CloudWatchLogsLogGroupArn"]
            .as_str()
            .map(String::from),
        cloud_watch_logs_role_arn: input["CloudWatchLogsRoleArn"].as_str().map(String::from),
        kms_key_id: input["KmsKeyId"].as_str().map(String::from),
        has_custom_event_selectors: false,
        has_insight_selectors: false,
        is_organization_trail: input["IsOrganizationTrail"].as_bool().unwrap_or(false),
    };
    state.trails.insert(name.to_string(), trail.clone());
    state.trail_status.insert(
        name.to_string(),
        TrailStatus {
            is_logging: false,
            latest_delivery_error: None,
            latest_notification_error: None,
            latest_delivery_time: None,
            latest_notification_time: None,
            start_logging_time: None,
            stop_logging_time: None,
        },
    );

    Ok(serialize_trail(&trail))
}

pub fn describe_trails(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let requested: Option<Vec<String>> = input["trailNameList"].as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });
    let trails: Vec<Value> = state
        .trails
        .iter()
        .filter(|e| {
            requested
                .as_ref()
                .map(|r| r.is_empty() || r.contains(e.key()) || r.contains(&e.value().arn))
                .unwrap_or(true)
        })
        .map(|e| serialize_trail_raw(e.value()))
        .collect();
    Ok(json!({ "trailList": trails }))
}

pub fn delete_trail(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let key = resolve_name(name);
    state.trails.remove(&key).ok_or_else(|| {
        AwsError::not_found("TrailNotFoundException", format!("Trail {key} not found"))
    })?;
    state.trail_status.remove(&key);
    state.event_selectors.remove(&key);
    state.insight_selectors.remove(&key);
    Ok(json!({}))
}

pub fn update_trail(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let key = resolve_name(name);
    let mut trail = state.trails.get_mut(&key).ok_or_else(|| {
        AwsError::not_found("TrailNotFoundException", format!("Trail {key} not found"))
    })?;
    if let Some(b) = input["S3BucketName"].as_str() {
        trail.s3_bucket_name = b.to_string();
    }
    if let Some(p) = input["S3KeyPrefix"].as_str() {
        trail.s3_key_prefix = Some(p.to_string());
    }
    if let Some(b) = input["IncludeGlobalServiceEvents"].as_bool() {
        trail.include_global_service_events = b;
    }
    if let Some(b) = input["IsMultiRegionTrail"].as_bool() {
        trail.is_multi_region_trail = b;
    }
    if let Some(b) = input["EnableLogFileValidation"].as_bool() {
        trail.log_file_validation_enabled = b;
    }
    let cloned = trail.clone();
    drop(trail);
    Ok(serialize_trail(&cloned))
}

pub fn start_logging(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let key = resolve_name(name);
    let mut s = state.trail_status.get_mut(&key).ok_or_else(|| {
        AwsError::not_found("TrailNotFoundException", format!("Trail {key} not found"))
    })?;
    s.is_logging = true;
    s.start_logging_time = Some(now_secs());
    Ok(json!({}))
}

pub fn stop_logging(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let key = resolve_name(name);
    let mut s = state.trail_status.get_mut(&key).ok_or_else(|| {
        AwsError::not_found("TrailNotFoundException", format!("Trail {key} not found"))
    })?;
    s.is_logging = false;
    s.stop_logging_time = Some(now_secs());
    Ok(json!({}))
}

pub fn get_trail_status(
    state: &CloudTrailState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Name is required"))?;
    let key = resolve_name(name);
    let s = state.trail_status.get(&key).ok_or_else(|| {
        AwsError::not_found("TrailNotFoundException", format!("Trail {key} not found"))
    })?;
    Ok(json!({
        "IsLogging": s.is_logging,
        "LatestDeliveryError": s.latest_delivery_error,
        "LatestNotificationError": s.latest_notification_error,
        "LatestDeliveryTime": s.latest_delivery_time,
        "LatestNotificationTime": s.latest_notification_time,
        "StartLoggingTime": s.start_logging_time,
        "StopLoggingTime": s.stop_logging_time,
    }))
}

pub fn list_trails(
    state: &CloudTrailState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let trails: Vec<Value> = state
        .trails
        .iter()
        .map(|e| {
            let t = e.value();
            json!({
                "TrailARN": t.arn,
                "Name": t.name,
                "HomeRegion": ctx.region,
            })
        })
        .collect();
    Ok(json!({ "Trails": trails }))
}

pub fn lookup_events(
    _state: &CloudTrailState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "Events": [] }))
}

fn resolve_name(s: &str) -> String {
    if let Some(idx) = s.rfind(':') {
        let rest = &s[idx + 1..];
        if let Some(n) = rest.strip_prefix("trail/") {
            return n.to_string();
        }
    }
    s.to_string()
}

fn serialize_trail_raw(t: &Trail) -> Value {
    json!({
        "Name": t.name,
        "S3BucketName": t.s3_bucket_name,
        "S3KeyPrefix": t.s3_key_prefix,
        "SnsTopicName": t.sns_topic_name,
        "SnsTopicARN": t.sns_topic_arn,
        "IncludeGlobalServiceEvents": t.include_global_service_events,
        "IsMultiRegionTrail": t.is_multi_region_trail,
        "HomeRegion": t.home_region,
        "TrailARN": t.arn,
        "LogFileValidationEnabled": t.log_file_validation_enabled,
        "CloudWatchLogsLogGroupArn": t.cloud_watch_logs_log_group_arn,
        "CloudWatchLogsRoleArn": t.cloud_watch_logs_role_arn,
        "KmsKeyId": t.kms_key_id,
        "HasCustomEventSelectors": t.has_custom_event_selectors,
        "HasInsightSelectors": t.has_insight_selectors,
        "IsOrganizationTrail": t.is_organization_trail,
    })
}

fn serialize_trail(t: &Trail) -> Value {
    json!({
        "Name": t.name,
        "S3BucketName": t.s3_bucket_name,
        "S3KeyPrefix": t.s3_key_prefix,
        "SnsTopicName": t.sns_topic_name,
        "SnsTopicARN": t.sns_topic_arn,
        "IncludeGlobalServiceEvents": t.include_global_service_events,
        "IsMultiRegionTrail": t.is_multi_region_trail,
        "TrailARN": t.arn,
        "LogFileValidationEnabled": t.log_file_validation_enabled,
        "CloudWatchLogsLogGroupArn": t.cloud_watch_logs_log_group_arn,
        "CloudWatchLogsRoleArn": t.cloud_watch_logs_role_arn,
        "KmsKeyId": t.kms_key_id,
        "IsOrganizationTrail": t.is_organization_trail,
    })
}
