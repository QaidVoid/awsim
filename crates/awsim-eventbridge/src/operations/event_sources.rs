use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EventBridgeState;

// ---------------------------------------------------------------------------
// DescribeEventSource — stub
// ---------------------------------------------------------------------------

pub fn describe_event_source(
    _state: &EventBridgeState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    Ok(json!({
        "Name": name,
        "Arn": format!("arn:aws:events:::event-source/{}", name),
        "State": "ACTIVE",
        "CreatedBy": "aws",
        "CreationTime": "0",
        "ExpirationTime": null,
    }))
}

// ---------------------------------------------------------------------------
// ListEventSources — stub returning empty list
// ---------------------------------------------------------------------------

pub fn list_event_sources(
    _state: &EventBridgeState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "EventSources": [] }))
}

// ---------------------------------------------------------------------------
// PutPartnerEventSource — stub
// ---------------------------------------------------------------------------

pub fn put_partner_event_source(
    _state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let arn = format!(
        "arn:aws:events:{}:{}:event-source/{}",
        ctx.region, ctx.account_id, name
    );

    Ok(json!({ "EventSourceArn": arn }))
}
