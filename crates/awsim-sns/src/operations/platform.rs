use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{PlatformApplication, PlatformEndpoint, SnsState};

// ---------------------------------------------------------------------------
// CreatePlatformApplication
// ---------------------------------------------------------------------------

pub fn create_platform_application(
    state: &SnsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;
    let platform = input["Platform"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Platform is required"))?;

    let arn = format!(
        "arn:aws:sns:{}:{}:app/{}/{}",
        ctx.region, ctx.account_id, platform, name
    );

    let mut attributes: HashMap<String, String> = HashMap::new();
    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    let app = PlatformApplication {
        arn: arn.clone(),
        platform: platform.to_string(),
        attributes,
    };

    state.platform_applications.insert(arn.clone(), app);
    Ok(json!({ "PlatformApplicationArn": arn }))
}

// ---------------------------------------------------------------------------
// DeletePlatformApplication
// ---------------------------------------------------------------------------

pub fn delete_platform_application(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PlatformApplicationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "PlatformApplicationArn is required")
    })?;

    state.platform_applications.remove(arn);
    // Also remove all endpoints for this application
    state
        .platform_endpoints
        .retain(|_, e| e.platform_application_arn != arn);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListPlatformApplications
// ---------------------------------------------------------------------------

pub fn list_platform_applications(
    state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let apps: Vec<Value> = state
        .platform_applications
        .iter()
        .map(|entry| {
            let app = entry.value();
            json!({
                "PlatformApplicationArn": app.arn,
                "Attributes": app.attributes,
            })
        })
        .collect();

    Ok(json!({ "PlatformApplications": apps }))
}

// ---------------------------------------------------------------------------
// GetPlatformApplicationAttributes
// ---------------------------------------------------------------------------

pub fn get_platform_application_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PlatformApplicationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "PlatformApplicationArn is required")
    })?;

    let app = state.platform_applications.get(arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Platform application not found: {arn}"))
    })?;

    let attrs: serde_json::Map<String, Value> = app
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "Attributes": attrs }))
}

// ---------------------------------------------------------------------------
// SetPlatformApplicationAttributes
// ---------------------------------------------------------------------------

pub fn set_platform_application_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["PlatformApplicationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "PlatformApplicationArn is required")
    })?;

    let mut app = state.platform_applications.get_mut(arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Platform application not found: {arn}"))
    })?;

    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                app.attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// CreatePlatformEndpoint
// ---------------------------------------------------------------------------

pub fn create_platform_endpoint(
    state: &SnsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let platform_application_arn = input["PlatformApplicationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "PlatformApplicationArn is required")
    })?;
    let token = input["Token"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Token is required"))?;

    // Derive platform from ARN: arn:aws:sns:{region}:{account}:app/{platform}/{name}
    let platform = platform_application_arn.split('/').nth(1).unwrap_or("APNS");

    let endpoint_arn = format!(
        "arn:aws:sns:{}:{}:endpoint/{}/{}/{}",
        ctx.region,
        ctx.account_id,
        platform,
        // extract app name from platform ARN
        platform_application_arn.rsplit('/').next().unwrap_or("app"),
        uuid::Uuid::new_v4()
    );

    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("Enabled".to_string(), "true".to_string());
    attributes.insert("Token".to_string(), token.to_string());

    if let Some(custom_data) = input["CustomUserData"].as_str() {
        attributes.insert("CustomUserData".to_string(), custom_data.to_string());
    }

    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    let endpoint = PlatformEndpoint {
        arn: endpoint_arn.clone(),
        platform_application_arn: platform_application_arn.to_string(),
        token: token.to_string(),
        attributes,
    };

    state
        .platform_endpoints
        .insert(endpoint_arn.clone(), endpoint);
    Ok(json!({ "EndpointArn": endpoint_arn }))
}

// ---------------------------------------------------------------------------
// DeleteEndpoint
// ---------------------------------------------------------------------------

pub fn delete_endpoint(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["EndpointArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EndpointArn is required"))?;

    state.platform_endpoints.remove(arn);
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListEndpointsByPlatformApplication
// ---------------------------------------------------------------------------

pub fn list_endpoints_by_platform_application(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let platform_application_arn = input["PlatformApplicationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "PlatformApplicationArn is required")
    })?;

    let endpoints: Vec<Value> = state
        .platform_endpoints
        .iter()
        .filter(|entry| entry.value().platform_application_arn == platform_application_arn)
        .map(|entry| {
            let e = entry.value();
            let attrs: serde_json::Map<String, Value> = e
                .attributes
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            json!({
                "EndpointArn": e.arn,
                "Attributes": attrs,
            })
        })
        .collect();

    Ok(json!({ "Endpoints": endpoints }))
}

// ---------------------------------------------------------------------------
// GetEndpointAttributes
// ---------------------------------------------------------------------------

pub fn get_endpoint_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["EndpointArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EndpointArn is required"))?;

    let endpoint = state
        .platform_endpoints
        .get(arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Endpoint not found: {arn}")))?;

    let attrs: serde_json::Map<String, Value> = endpoint
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "Attributes": attrs }))
}

// ---------------------------------------------------------------------------
// SetEndpointAttributes
// ---------------------------------------------------------------------------

pub fn set_endpoint_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["EndpointArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EndpointArn is required"))?;

    let mut endpoint = state
        .platform_endpoints
        .get_mut(arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Endpoint not found: {arn}")))?;

    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                endpoint.attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// OptInPhoneNumber
// ---------------------------------------------------------------------------

pub fn opt_in_phone_number(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let phone_number = input["phoneNumber"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "phoneNumber is required"))?;

    let mut numbers = state
        .opted_in_numbers
        .write()
        .map_err(|_| AwsError::internal("Failed to acquire opted-in numbers lock"))?;

    if !numbers.contains(&phone_number.to_string()) {
        numbers.push(phone_number.to_string());
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListOriginationNumbers
// ---------------------------------------------------------------------------

pub fn list_origination_numbers(
    _state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Stub — no origination numbers in simulation.
    Ok(json!({ "PhoneNumbers": [] }))
}
