use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{EventBridgeState, EventBus};

// ---------------------------------------------------------------------------
// CreateEventBus
// ---------------------------------------------------------------------------

pub fn create_event_bus(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    if name.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Event bus name must not be empty",
        ));
    }

    if name == "default" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Cannot create an event bus named 'default'",
        ));
    }

    if state.event_buses.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceAlreadyExistsException",
            format!("Event bus {name} already exists"),
        ));
    }

    let arn = arn::build(ctx, "events", format!("event-bus/{name}"));

    let bus = EventBus::new(name.to_string(), arn.clone());
    state.event_buses.insert(name.to_string(), bus);

    info!(bus = %name, "Created event bus");
    Ok(json!({ "EventBusArn": arn }))
}

// ---------------------------------------------------------------------------
// DeleteEventBus
// ---------------------------------------------------------------------------

pub fn delete_event_bus(
    state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Name is required"))?;

    if name == "default" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            "Cannot delete the default event bus",
        ));
    }

    state.event_buses.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {name} does not exist"),
        )
    })?;

    info!(bus = %name, "Deleted event bus");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeEventBus
// ---------------------------------------------------------------------------

pub fn describe_event_bus(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"].as_str().unwrap_or("default");

    ensure_default_bus(state, ctx);

    let bus = state.event_buses.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {name} does not exist"),
        )
    })?;

    let mut obj = json!({
        "Name": bus.name,
        "Arn": bus.arn,
    });
    if let Some(ref p) = bus.policy {
        obj["Policy"] = json!(p);
    }
    Ok(obj)
}

// ---------------------------------------------------------------------------
// PutPermission / RemovePermission — manages the resource policy that
// guards cross-account PutEvents. AWS accepts either a flat
// `{Action, Principal, StatementId}` triplet or a full `Policy`
// document; we follow the same shape and persist the policy verbatim
// so DescribeEventBus echoes it back.
// ---------------------------------------------------------------------------

pub fn put_permission(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["EventBusName"].as_str().unwrap_or("default");
    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {name} does not exist"),
        )
    })?;

    // Accept either an explicit `Policy` JSON string or a structured
    // statement that we splice into a fresh / existing policy doc.
    if let Some(policy) = input.get("Policy").and_then(Value::as_str) {
        if serde_json::from_str::<Value>(policy).is_err() {
            return Err(AwsError::bad_request(
                "InvalidPolicyException",
                "Policy must be a JSON document.",
            ));
        }
        bus.policy = Some(policy.to_string());
        return Ok(json!({}));
    }

    let action = input["Action"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            "Action is required when Policy is absent",
        )
    })?;
    let principal = input["Principal"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            "Principal is required when Policy is absent",
        )
    })?;
    let statement_id = input["StatementId"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            "StatementId is required when Policy is absent",
        )
    })?;

    // Append a fresh statement, replacing any existing entry with the
    // same Sid (AWS treats StatementId as the unique key).
    let mut doc: Value = bus
        .policy
        .as_ref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| json!({"Version":"2012-10-17","Statement":[]}));
    let stmts = doc["Statement"]
        .as_array_mut()
        .ok_or_else(|| AwsError::internal("Event bus policy missing Statement[]"))?;
    stmts.retain(|s| s.get("Sid").and_then(Value::as_str) != Some(statement_id));
    let principal_value = if principal == "*" {
        json!("*")
    } else {
        json!({ "AWS": format!("arn:{}:iam::{principal}:root", ctx.partition) })
    };
    stmts.push(json!({
        "Sid": statement_id,
        "Effect": "Allow",
        "Principal": principal_value,
        "Action": action,
        "Resource": bus.arn,
    }));
    bus.policy = Some(doc.to_string());
    Ok(json!({}))
}

pub fn remove_permission(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["EventBusName"].as_str().unwrap_or("default");
    ensure_default_bus(state, ctx);

    let mut bus = state.event_buses.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Event bus {name} does not exist"),
        )
    })?;
    let statement_id = input["StatementId"].as_str();
    let remove_all = input["RemoveAllPermissions"].as_bool().unwrap_or(false);
    if remove_all {
        bus.policy = None;
        return Ok(json!({}));
    }
    let Some(sid) = statement_id else {
        return Err(AwsError::bad_request(
            "ValidationException",
            "StatementId is required when RemoveAllPermissions is false",
        ));
    };
    let Some(raw) = bus.policy.clone() else {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Statement {sid} not found"),
        ));
    };
    let mut doc: Value = serde_json::from_str(&raw)
        .map_err(|_| AwsError::internal("Stored event bus policy is malformed"))?;
    let stmts = doc["Statement"]
        .as_array_mut()
        .ok_or_else(|| AwsError::internal("Event bus policy missing Statement[]"))?;
    let before = stmts.len();
    stmts.retain(|s| s.get("Sid").and_then(Value::as_str) != Some(sid));
    if stmts.len() == before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Statement {sid} not found"),
        ));
    }
    if stmts.is_empty() {
        bus.policy = None;
    } else {
        bus.policy = Some(doc.to_string());
    }
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListEventBuses
// ---------------------------------------------------------------------------

pub fn list_event_buses(
    state: &EventBridgeState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    ensure_default_bus(state, ctx);

    let buses: Vec<Value> = state
        .event_buses
        .iter()
        .map(|entry| {
            json!({
                "Name": entry.value().name,
                "Arn": entry.value().arn,
            })
        })
        .collect();

    Ok(json!({ "EventBuses": buses }))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ensure the "default" event bus exists for this account/region.
/// Called lazily on first access.
pub fn ensure_default_bus(state: &EventBridgeState, ctx: &RequestContext) {
    if !state.event_buses.contains_key("default") {
        let arn = arn::build(ctx, "events", "event-bus/default");
        state.event_buses.insert(
            "default".to_string(),
            EventBus::new("default".to_string(), arn),
        );
    }
}
