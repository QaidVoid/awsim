use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::{CloudTrailState, EventSelector, InsightSelector};

pub fn get_event_selectors(
    state: &CloudTrailState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TrailName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TrailName is required"))?;
    let key = resolve_name(name);
    let trail_arn = arn::build(ctx, "cloudtrail", format!("trail/{key}"));
    let selectors: Vec<Value> = state
        .event_selectors
        .get(&key)
        .map(|v| {
            v.iter()
                .map(|s| {
                    json!({
                        "ReadWriteType": s.read_write_type,
                        "IncludeManagementEvents": s.include_management_events,
                        "DataResources": s.data_resources,
                        "ExcludeManagementEventSources": s.exclude_management_event_sources,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "TrailARN": trail_arn,
        "EventSelectors": selectors,
    }))
}

pub fn put_event_selectors(
    state: &CloudTrailState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TrailName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TrailName is required"))?;
    let key = resolve_name(name);
    let trail_arn = arn::build(ctx, "cloudtrail", format!("trail/{key}"));
    let sels: Vec<EventSelector> = input["EventSelectors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| EventSelector {
                    read_write_type: v["ReadWriteType"].as_str().unwrap_or("All").to_string(),
                    include_management_events: v["IncludeManagementEvents"]
                        .as_bool()
                        .unwrap_or(true),
                    data_resources: v["DataResources"].as_array().cloned().unwrap_or_default(),
                    exclude_management_event_sources: v["ExcludeManagementEventSources"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let returned = input["EventSelectors"].clone();
    state.event_selectors.insert(key, sels);
    Ok(json!({
        "TrailARN": trail_arn,
        "EventSelectors": returned,
    }))
}

pub fn put_insight_selectors(
    state: &CloudTrailState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TrailName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TrailName is required"))?;
    let key = resolve_name(name);
    let trail_arn = arn::build(ctx, "cloudtrail", format!("trail/{key}"));
    let sels: Vec<InsightSelector> = input["InsightSelectors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| InsightSelector {
                    insight_type: v["InsightType"]
                        .as_str()
                        .unwrap_or("ApiCallRateInsight")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let returned = input["InsightSelectors"].clone();
    state.insight_selectors.insert(key, sels);
    Ok(json!({
        "TrailARN": trail_arn,
        "InsightSelectors": returned,
    }))
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
