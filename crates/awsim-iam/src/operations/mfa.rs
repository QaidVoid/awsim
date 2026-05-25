use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{entity_already_exists, no_such_entity},
    ids::{new_base32_seed, normalize_path, now_iso8601},
    state::{IamState, VirtualMfaDevice},
};

use super::super::operations::tags::{parse_tag_keys, parse_tags, tags_to_value};
use super::{opt_str, require_str};

fn device_to_value(d: &VirtualMfaDevice) -> Value {
    let mut v = json!({
        "SerialNumber": d.serial_number,
    });
    if let Some(seed) = &d.base32_string_seed {
        v["Base32StringSeed"] = Value::String(seed.clone());
    }
    if let Some(user) = &d.user {
        v["User"] = json!({ "UserName": user });
    }
    if let Some(ed) = &d.enable_date {
        v["EnableDate"] = Value::String(ed.clone());
    }
    v
}

pub fn create_virtual_mfa_device(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let device_name = require_str(input, "VirtualMFADeviceName")?;
    let path = normalize_path(opt_str(input, "Path"));
    let tags = parse_tags(input)?;

    let serial_number = format!("arn:aws:iam::{}:mfa{}{}", ctx.account_id, path, device_name);

    if state.virtual_mfa_devices.contains_key(&serial_number) {
        return Err(entity_already_exists("VirtualMFADevice", device_name));
    }

    let device = VirtualMfaDevice {
        serial_number: serial_number.clone(),
        base32_string_seed: Some(new_base32_seed()),
        qr_code_png: None,
        user: None,
        enable_date: None,
        tags,
    };

    let result = device_to_value(&device);
    state.virtual_mfa_devices.insert(serial_number, device);

    Ok(json!({ "VirtualMFADevice": result }))
}

pub fn list_virtual_mfa_devices(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let assignment_status = opt_str(input, "AssignmentStatus").unwrap_or("Any");

    let devices: Vec<Value> = state
        .virtual_mfa_devices
        .iter()
        .filter(|d| match assignment_status {
            "Assigned" => d.user.is_some(),
            "Unassigned" => d.user.is_none(),
            _ => true, // "Any"
        })
        .map(|d| device_to_value(&d))
        .collect();

    Ok(json!({
        "VirtualMFADevices": { "member": devices },
        "IsTruncated": false,
    }))
}

pub fn delete_virtual_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let serial_number = require_str(input, "SerialNumber")?;

    if state.virtual_mfa_devices.remove(serial_number).is_none() {
        return Err(no_such_entity("VirtualMFADevice", serial_number));
    }

    Ok(json!({}))
}

pub fn enable_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let serial_number = require_str(input, "SerialNumber")?;
    let code1 = parse_auth_code(input, "AuthenticationCode1")?;
    let code2 = parse_auth_code(input, "AuthenticationCode2")?;

    // Ensure user exists
    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    // Ensure device exists, then verify the two consecutive TOTP
    // codes match the seed we issued at CreateVirtualMFADevice.
    {
        let device = state
            .virtual_mfa_devices
            .get(serial_number)
            .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
        let seed = device.base32_string_seed.as_deref().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidAuthenticationCode",
                "MFA device has no seed; recreate the device.",
            )
        })?;
        if !awsim_core::totp::verify_consecutive(
            seed,
            code1,
            code2,
            std::time::SystemTime::now(),
            1,
        ) {
            return Err(AwsError::bad_request(
                "InvalidAuthenticationCode",
                "AuthenticationCode1 and AuthenticationCode2 must be two consecutive valid TOTP codes for the device.",
            ));
        }
    }

    {
        let mut device = state
            .virtual_mfa_devices
            .get_mut(serial_number)
            .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
        device.user = Some(user_name.to_string());
        device.enable_date = Some(now_iso8601());
    }

    {
        let mut user = state
            .users
            .get_mut(user_name)
            .ok_or_else(|| no_such_entity("User", user_name))?;
        if !user.mfa_devices.contains(&serial_number.to_string()) {
            user.mfa_devices.push(serial_number.to_string());
        }
    }

    Ok(json!({}))
}

fn parse_auth_code(input: &Value, field: &str) -> Result<u32, AwsError> {
    let raw = require_str(input, field)?;
    raw.parse::<u32>().map_err(|_| {
        AwsError::bad_request(
            "InvalidAuthenticationCode",
            format!("{field} must be a 6-digit number."),
        )
    })
}

pub fn deactivate_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let serial_number = require_str(input, "SerialNumber")?;

    // Unlink device from user
    {
        let mut device = state
            .virtual_mfa_devices
            .get_mut(serial_number)
            .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
        device.user = None;
        device.enable_date = None;
    }

    // Remove from user's list
    if let Some(mut user) = state.users.get_mut(user_name) {
        user.mfa_devices.retain(|s| s != serial_number);
    }

    Ok(json!({}))
}

pub fn get_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let serial_number = require_str(input, "SerialNumber")?;
    let device = state
        .virtual_mfa_devices
        .get(serial_number)
        .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;

    let mut v = json!({
        "SerialNumber": device.serial_number,
        "Certifications": { "entry": [] },
    });
    if let Some(user) = &device.user {
        v["UserName"] = Value::String(user.clone());
    }
    if let Some(ed) = &device.enable_date {
        v["EnableDate"] = Value::String(ed.clone());
    }
    Ok(v)
}

pub fn resync_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let _user_name = require_str(input, "UserName")?;
    let serial_number = require_str(input, "SerialNumber")?;
    let code1 = parse_auth_code(input, "AuthenticationCode1")?;
    let code2 = parse_auth_code(input, "AuthenticationCode2")?;

    let device = state
        .virtual_mfa_devices
        .get(serial_number)
        .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
    let seed = device.base32_string_seed.as_deref().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidAuthenticationCode",
            "MFA device has no seed; recreate the device.",
        )
    })?;
    // Use a wider window for resync since the whole point is the
    // device clock has drifted.
    if !awsim_core::totp::verify_consecutive(seed, code1, code2, std::time::SystemTime::now(), 4) {
        return Err(AwsError::bad_request(
            "InvalidAuthenticationCode",
            "AuthenticationCode1 and AuthenticationCode2 must be two consecutive valid TOTP codes for the device.",
        ));
    }
    Ok(json!({}))
}

pub fn tag_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let serial_number = require_str(input, "SerialNumber")?;
    let new_tags = parse_tags(input)?;

    let mut device = state
        .virtual_mfa_devices
        .get_mut(serial_number)
        .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
    for (k, v) in new_tags {
        device.tags.insert(k, v);
    }
    Ok(json!({}))
}

pub fn untag_mfa_device(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let serial_number = require_str(input, "SerialNumber")?;
    let keys = parse_tag_keys(input)?;

    let mut device = state
        .virtual_mfa_devices
        .get_mut(serial_number)
        .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
    for k in &keys {
        device.tags.remove(k);
    }
    Ok(json!({}))
}

pub fn list_mfa_device_tags(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let serial_number = require_str(input, "SerialNumber")?;
    let device = state
        .virtual_mfa_devices
        .get(serial_number)
        .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;

    Ok(json!({
        "Tags": tags_to_value(&device.tags),
        "IsTruncated": false,
    }))
}

pub fn list_mfa_devices(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;

    let user = state
        .users
        .get(user_name)
        .ok_or_else(|| no_such_entity("User", user_name))?;

    let devices: Vec<Value> = user
        .mfa_devices
        .iter()
        .filter_map(|sn| {
            state.virtual_mfa_devices.get(sn).map(|d| {
                json!({
                    "UserName": user_name,
                    "SerialNumber": d.serial_number,
                    "EnableDate": d.enable_date.clone().unwrap_or_default(),
                })
            })
        })
        .collect();

    Ok(json!({
        "MFADevices": { "member": devices },
        "IsTruncated": false,
    }))
}
