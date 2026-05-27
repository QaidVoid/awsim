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
        "Status": d.status,
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
        status: "Unassigned".to_string(),
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
        if device.status == "Active" {
            return Err(AwsError::conflict(
                "EntityAlreadyExistsException",
                format!("MFA device `{serial_number}` is already assigned and active."),
            ));
        }
        device.user = Some(user_name.to_string());
        device.enable_date = Some(now_iso8601());
        device.status = "Active".to_string();
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
        if device.status != "Active" {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("MFA device `{serial_number}` is not in the Active state."),
            ));
        }
        device.user = None;
        device.enable_date = None;
        device.status = "Unassigned".to_string();
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
        "Status": device.status,
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

    {
        let device = state
            .virtual_mfa_devices
            .get(serial_number)
            .ok_or_else(|| no_such_entity("VirtualMFADevice", serial_number))?;
        if device.status != "Active" {
            return Err(AwsError::bad_request(
                "InvalidInput",
                format!("MFA device `{serial_number}` is not in the Active state."),
            ));
        }
        let seed = device.base32_string_seed.as_deref().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidAuthenticationCode",
                "MFA device has no seed; recreate the device.",
            )
        })?;
        // Use a wider window for resync since the whole point is the
        // device clock has drifted.
        if !awsim_core::totp::verify_consecutive(
            seed,
            code1,
            code2,
            std::time::SystemTime::now(),
            4,
        ) {
            return Err(AwsError::bad_request(
                "InvalidAuthenticationCode",
                "AuthenticationCode1 and AuthenticationCode2 must be two consecutive valid TOTP codes for the device.",
            ));
        }
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

#[cfg(test)]
mod state_machine_tests {
    use super::*;
    use crate::state::{IamState, User};
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn ctx() -> RequestContext {
        RequestContext::new("iam", "us-east-1")
    }

    fn seed_user(state: &IamState, name: &str) {
        state.users.insert(
            name.to_string(),
            User {
                user_name: name.into(),
                user_id: "AIDAFAKE00000000000A".into(),
                arn: format!("arn:aws:iam::000000000000:user/{name}"),
                path: "/".into(),
                create_date: "1970-01-01T00:00:00Z".into(),
                access_keys: vec![],
                attached_policies: vec![],
                inline_policies: HashMap::new(),
                groups: vec![],
                tags: HashMap::new(),
                mfa_devices: vec![],
                ssh_public_keys: vec![],
                password_last_used: None,
            },
        );
    }

    fn current_two_codes(seed: &str) -> (u32, u32) {
        let secret = awsim_core::totp::decode_base32(seed).expect("base32 seed");
        let now_secs = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let c1 = awsim_core::totp::code_at(&secret, now_secs);
        let c2 = awsim_core::totp::code_at(&secret, now_secs + 30);
        (c1, c2)
    }

    #[test]
    fn newly_created_device_is_unassigned() {
        let state = IamState::default();
        let resp =
            create_virtual_mfa_device(&state, &json!({ "VirtualMFADeviceName": "phone" }), &ctx())
                .unwrap();
        assert_eq!(resp["VirtualMFADevice"]["Status"], "Unassigned");
    }

    #[test]
    fn enable_with_valid_codes_moves_to_active() {
        let state = IamState::default();
        seed_user(&state, "alice");
        let create =
            create_virtual_mfa_device(&state, &json!({ "VirtualMFADeviceName": "phone" }), &ctx())
                .unwrap();
        let serial = create["VirtualMFADevice"]["SerialNumber"]
            .as_str()
            .unwrap()
            .to_string();
        let seed = create["VirtualMFADevice"]["Base32StringSeed"]
            .as_str()
            .unwrap()
            .to_string();
        let (c1, c2) = current_two_codes(&seed);

        enable_mfa_device(
            &state,
            &json!({
                "UserName": "alice",
                "SerialNumber": serial,
                "AuthenticationCode1": format!("{:06}", c1),
                "AuthenticationCode2": format!("{:06}", c2),
            }),
        )
        .unwrap();

        let got = get_mfa_device(&state, &json!({ "SerialNumber": serial })).unwrap();
        assert_eq!(got["Status"], "Active");
        assert_eq!(got["UserName"], "alice");
    }

    #[test]
    fn enable_rejects_already_active_device() {
        let state = IamState::default();
        seed_user(&state, "alice");
        let create =
            create_virtual_mfa_device(&state, &json!({ "VirtualMFADeviceName": "phone" }), &ctx())
                .unwrap();
        let serial = create["VirtualMFADevice"]["SerialNumber"]
            .as_str()
            .unwrap()
            .to_string();
        let seed = create["VirtualMFADevice"]["Base32StringSeed"]
            .as_str()
            .unwrap()
            .to_string();
        let (c1, c2) = current_two_codes(&seed);

        enable_mfa_device(
            &state,
            &json!({
                "UserName": "alice",
                "SerialNumber": serial,
                "AuthenticationCode1": format!("{:06}", c1),
                "AuthenticationCode2": format!("{:06}", c2),
            }),
        )
        .unwrap();

        let err = enable_mfa_device(
            &state,
            &json!({
                "UserName": "alice",
                "SerialNumber": serial,
                "AuthenticationCode1": format!("{:06}", c1),
                "AuthenticationCode2": format!("{:06}", c2),
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, "EntityAlreadyExistsException");
    }

    #[test]
    fn deactivate_returns_device_to_unassigned() {
        let state = IamState::default();
        seed_user(&state, "alice");
        let create =
            create_virtual_mfa_device(&state, &json!({ "VirtualMFADeviceName": "phone" }), &ctx())
                .unwrap();
        let serial = create["VirtualMFADevice"]["SerialNumber"]
            .as_str()
            .unwrap()
            .to_string();
        let seed = create["VirtualMFADevice"]["Base32StringSeed"]
            .as_str()
            .unwrap()
            .to_string();
        let (c1, c2) = current_two_codes(&seed);
        enable_mfa_device(
            &state,
            &json!({
                "UserName": "alice",
                "SerialNumber": serial,
                "AuthenticationCode1": format!("{:06}", c1),
                "AuthenticationCode2": format!("{:06}", c2),
            }),
        )
        .unwrap();

        deactivate_mfa_device(
            &state,
            &json!({ "UserName": "alice", "SerialNumber": serial }),
        )
        .unwrap();
        let got = get_mfa_device(&state, &json!({ "SerialNumber": serial })).unwrap();
        assert_eq!(got["Status"], "Unassigned");
    }

    #[test]
    fn deactivate_rejects_unassigned_device() {
        let state = IamState::default();
        let create =
            create_virtual_mfa_device(&state, &json!({ "VirtualMFADeviceName": "phone" }), &ctx())
                .unwrap();
        let serial = create["VirtualMFADevice"]["SerialNumber"]
            .as_str()
            .unwrap()
            .to_string();
        let err = deactivate_mfa_device(
            &state,
            &json!({ "UserName": "alice", "SerialNumber": serial }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInput");
    }
}
