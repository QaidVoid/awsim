use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SnsState;

// ---------------------------------------------------------------------------
// CheckIfPhoneNumberIsOptedOut
// ---------------------------------------------------------------------------

/// Stub: always returns isOptedOut = false.
pub fn check_if_phone_number_is_opted_out(
    _state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "isOptedOut": false }))
}

// ---------------------------------------------------------------------------
// ListPhoneNumbersOptedOut
// ---------------------------------------------------------------------------

/// Stub: always returns an empty list.
pub fn list_phone_numbers_opted_out(
    _state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "phoneNumbers": [] }))
}

// ---------------------------------------------------------------------------
// GetSMSAttributes
// ---------------------------------------------------------------------------

pub fn get_sms_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let attrs = state
        .sms_attributes
        .read()
        .map_err(|_| AwsError::internal("Failed to acquire SMS attributes lock"))?;

    // If specific attribute names are requested, filter; otherwise return all.
    let filter: Option<Vec<&str>> = input["attributes"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());

    let mut result = serde_json::Map::new();

    // Provide defaults for the standard SMS attributes if not explicitly set.
    let defaults = default_sms_attributes();
    for (k, v) in &defaults {
        if filter.as_ref().map_or(true, |f| f.contains(&k.as_str())) {
            let val = attrs.get(k).cloned().unwrap_or_else(|| v.clone());
            result.insert(k.clone(), Value::String(val));
        }
    }

    // Include any extra attributes stored beyond the defaults.
    for (k, v) in attrs.iter() {
        if !defaults.contains_key(k) {
            if filter.as_ref().map_or(true, |f| f.contains(&k.as_str())) {
                result.insert(k.clone(), Value::String(v.clone()));
            }
        }
    }

    Ok(json!({ "attributes": result }))
}

// ---------------------------------------------------------------------------
// SetSMSAttributes
// ---------------------------------------------------------------------------

pub fn set_sms_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let attrs = input["attributes"]
        .as_object()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "attributes is required"))?;

    let mut sms_attrs = state
        .sms_attributes
        .write()
        .map_err(|_| AwsError::internal("Failed to acquire SMS attributes lock"))?;

    for (k, v) in attrs {
        if let Some(s) = v.as_str() {
            sms_attrs.insert(k.clone(), s.to_string());
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_sms_attributes() -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    m.insert("MonthlySpendLimit".to_string(), "1".to_string());
    m.insert("DeliveryStatusIAMRole".to_string(), String::new());
    m.insert(
        "DeliveryStatusSuccessSamplingRate".to_string(),
        "0".to_string(),
    );
    m.insert("DefaultSenderID".to_string(), String::new());
    m.insert("DefaultSMSType".to_string(), "Transactional".to_string());
    m.insert(
        "UsageReportS3Bucket".to_string(),
        String::new(),
    );
    m
}
