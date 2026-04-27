use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{SandboxPhoneNumber, SnsState};

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
        if filter.as_ref().is_none_or(|f| f.contains(&k.as_str())) {
            let val = attrs.get(k).cloned().unwrap_or_else(|| v.clone());
            result.insert(k.clone(), Value::String(val));
        }
    }

    // Include any extra attributes stored beyond the defaults.
    for (k, v) in attrs.iter() {
        if !defaults.contains_key(k) && filter.as_ref().is_none_or(|f| f.contains(&k.as_str())) {
            result.insert(k.clone(), Value::String(v.clone()));
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

// ---------------------------------------------------------------------------
// CreateSMSSandboxPhoneNumber
// ---------------------------------------------------------------------------

pub fn create_sms_sandbox_phone_number(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let phone_number = input["PhoneNumber"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PhoneNumber is required"))?;
    let language_code = input["LanguageCode"]
        .as_str()
        .unwrap_or("en-US")
        .to_string();

    state.sandbox_numbers.insert(
        phone_number.to_string(),
        SandboxPhoneNumber {
            phone_number: phone_number.to_string(),
            status: "Pending".to_string(),
            language_code,
        },
    );

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteSMSSandboxPhoneNumber
// ---------------------------------------------------------------------------

pub fn delete_sms_sandbox_phone_number(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let phone_number = input["PhoneNumber"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PhoneNumber is required"))?;

    state.sandbox_numbers.remove(phone_number);
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// VerifySMSSandboxPhoneNumber
// ---------------------------------------------------------------------------

pub fn verify_sms_sandbox_phone_number(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let phone_number = input["PhoneNumber"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PhoneNumber is required"))?;
    let _otp = input["OneTimePassword"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OneTimePassword is required"))?;

    if let Some(mut entry) = state.sandbox_numbers.get_mut(phone_number) {
        entry.status = "Verified".to_string();
        Ok(json!({}))
    } else {
        Err(AwsError::not_found(
            "ResourceNotFound",
            format!("Phone number not found: {phone_number}"),
        ))
    }
}

// ---------------------------------------------------------------------------
// ListSMSSandboxPhoneNumbers
// ---------------------------------------------------------------------------

pub fn list_sms_sandbox_phone_numbers(
    state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let numbers: Vec<Value> = state
        .sandbox_numbers
        .iter()
        .map(|entry| {
            let n = entry.value();
            json!({
                "PhoneNumber": n.phone_number,
                "Status": n.status,
            })
        })
        .collect();

    Ok(json!({ "PhoneNumbers": numbers }))
}

// ---------------------------------------------------------------------------
// GetSMSSandboxAccountStatus
// ---------------------------------------------------------------------------

pub fn get_sms_sandbox_account_status(
    _state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "IsInSandbox": true }))
}

// ---------------------------------------------------------------------------
// GetDataProtectionPolicy
// ---------------------------------------------------------------------------

pub fn get_data_protection_policy(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;

    let policy = state
        .data_protection_policies
        .get(resource_arn)
        .map(|e| e.value().clone())
        .unwrap_or_default();

    Ok(json!({ "DataProtectionPolicy": policy }))
}

// ---------------------------------------------------------------------------
// PutDataProtectionPolicy
// ---------------------------------------------------------------------------

pub fn put_data_protection_policy(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["ResourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ResourceArn is required"))?;
    let policy = input["DataProtectionPolicy"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "DataProtectionPolicy is required")
    })?;

    if !state.topics.contains_key(resource_arn) {
        return Err(AwsError::not_found(
            "NotFound",
            format!("Topic not found: {resource_arn}"),
        ));
    }

    state
        .data_protection_policies
        .insert(resource_arn.to_string(), policy.to_string());

    Ok(json!({}))
}

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
    m.insert("UsageReportS3Bucket".to_string(), String::new());
    m
}
