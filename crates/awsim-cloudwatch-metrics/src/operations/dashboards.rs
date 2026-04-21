use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CloudWatchState, Dashboard};

/// PutDashboard
pub fn put_dashboard(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("DashboardName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValue", "DashboardName is required")
        })?
        .to_string();
    let body = input
        .get("DashboardBody")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();

    state
        .dashboards
        .insert(name.clone(), Dashboard { name, body });

    Ok(json!({ "DashboardValidationMessages": [] }))
}

/// GetDashboard
pub fn get_dashboard(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("DashboardName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValue", "DashboardName is required")
        })?;

    let dashboard = state
        .dashboards
        .get(name)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Dashboard {name} not found"),
            )
        })?;

    Ok(json!({
        "DashboardName": dashboard.name,
        "DashboardBody": dashboard.body,
        "DashboardArn": format!("arn:aws:cloudwatch::000000000000:dashboard/{}", dashboard.name),
    }))
}

/// ListDashboards
pub fn list_dashboards(
    state: &Arc<CloudWatchState>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let entries: Vec<Value> = state
        .dashboards
        .iter()
        .map(|entry| {
            json!({
                "DashboardName": entry.value().name,
                "DashboardArn": format!("arn:aws:cloudwatch::000000000000:dashboard/{}", entry.value().name),
            })
        })
        .collect();

    Ok(json!({ "DashboardEntries": entries }))
}

/// DeleteDashboards
pub fn delete_dashboards(
    state: &Arc<CloudWatchState>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<&str> = input
        .get("DashboardNames")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|n| n.as_str()).collect())
        .unwrap_or_default();

    for name in names {
        state.dashboards.remove(name);
    }

    Ok(json!({}))
}
