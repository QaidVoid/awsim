use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{HealthCheck, Route53State};

/// GET /2013-04-01/healthcheckcount
pub fn get_health_check_count(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "HealthCheckCount": state.health_checks.len()
    }))
}

/// GET /2013-04-01/healthcheck/{Id}/status
pub fn get_health_check_status(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("HealthCheckId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "HealthCheckId is required"))?;

    if !state.health_checks.contains_key(id) {
        return Err(AwsError::not_found(
            "NoSuchHealthCheck",
            format!("No health check found with ID: {id}"),
        ));
    }

    Ok(json!({
        "HealthCheckObservations": [
            {
                "Region": "us-east-1",
                "IPAddress": "198.51.100.1",
                "StatusReport": {
                    "Status": "Success: HTTP Status Code 200",
                    "CheckedTime": "2024-01-01T00:00:00Z",
                }
            },
            {
                "Region": "eu-west-1",
                "IPAddress": "198.51.100.2",
                "StatusReport": {
                    "Status": "Success: HTTP Status Code 200",
                    "CheckedTime": "2024-01-01T00:00:00Z",
                }
            }
        ]
    }))
}

/// POST /2013-04-01/healthcheck/{Id}
pub fn update_health_check(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("HealthCheckId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "HealthCheckId is required"))?;

    let mut hc = state
        .health_checks
        .get_mut(id)
        .ok_or_else(|| AwsError::not_found("NoSuchHealthCheck", format!("No health check found with ID: {id}")))?;

    // Merge any provided fields into the config
    if let Some(new_config) = input.get("HealthCheckConfig") {
        if let (Some(existing), Some(updates)) = (hc.config.as_object_mut(), new_config.as_object()) {
            for (k, v) in updates {
                existing.insert(k.clone(), v.clone());
            }
        }
    }
    hc.health_check_version += 1;
    let version = hc.health_check_version;
    let config = hc.config.clone();

    Ok(json!({
        "HealthCheck": {
            "Id": id,
            "HealthCheckVersion": version,
            "HealthCheckConfig": config,
        }
    }))
}

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
