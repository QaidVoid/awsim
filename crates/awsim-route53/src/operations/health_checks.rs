use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{HealthCheck, Route53State};

/// POST /2013-04-01/healthcheck
pub fn create_health_check(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let config = input
        .get("HealthCheckConfig")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let id = Uuid::new_v4().to_string();
    let hc = HealthCheck {
        id: id.clone(),
        config: config.clone(),
        health_check_version: 1,
    };

    state.health_checks.insert(id.clone(), hc);

    Ok(json!({
        "HealthCheck": {
            "Id": id,
            "HealthCheckVersion": 1,
            "HealthCheckConfig": config,
        }
    }))
}

/// GET /2013-04-01/healthcheck
pub fn list_health_checks(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let checks: Vec<Value> = state
        .health_checks
        .iter()
        .map(|entry| {
            let hc = entry.value();
            json!({
                "Id": hc.id,
                "HealthCheckVersion": hc.health_check_version,
                "HealthCheckConfig": hc.config,
            })
        })
        .collect();

    Ok(json!({
        "HealthChecks": checks,
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

/// DELETE /2013-04-01/healthcheck/{Id}
pub fn delete_health_check(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;

    if state.health_checks.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NoSuchHealthCheck",
            format!("No health check found with ID: {id}"),
        ));
    }

    Ok(json!({}))
}
