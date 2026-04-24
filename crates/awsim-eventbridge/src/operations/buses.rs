use awsim_core::{AwsError, RequestContext};
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

    let arn = format!(
        "arn:aws:events:{}:{}:event-bus/{}",
        ctx.region, ctx.account_id, name
    );

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

    Ok(json!({
        "Name": bus.name,
        "Arn": bus.arn,
    }))
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
        let arn = format!(
            "arn:aws:events:{}:{}:event-bus/default",
            ctx.region, ctx.account_id
        );
        state.event_buses.insert(
            "default".to_string(),
            EventBus::new("default".to_string(), arn),
        );
    }
}
