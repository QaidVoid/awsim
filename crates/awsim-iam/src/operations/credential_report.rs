use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    ids::{new_uuid, now_iso8601},
    state::IamState,
};

use super::require_str;

/// Generate and return a credential report instantly.
pub fn generate_credential_report(_state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({
        "State": "COMPLETE",
        "Description": "No report exists. Starting a new report generation task",
    }))
}

pub fn get_credential_report(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    // Build a CSV report of all users
    let mut csv = String::from(
        "user,arn,user_creation_time,password_enabled,password_last_used,password_last_changed,\
         password_next_rotation,mfa_active,access_key_1_active,access_key_1_last_rotated,\
         access_key_1_last_used_date,access_key_1_last_used_region,access_key_1_last_used_service,\
         access_key_2_active,access_key_2_last_rotated,access_key_2_last_used_date,\
         access_key_2_last_used_region,access_key_2_last_used_service,cert_1_active,\
         cert_1_last_rotated,cert_2_active,cert_2_last_rotated\n",
    );

    // Root account entry
    csv.push_str("<root_account>,arn:aws:iam::root,,,not_supported,,,false,false,N/A,N/A,N/A,N/A,false,N/A,N/A,N/A,N/A,false,N/A,false,N/A\n");

    for entry in state.users.iter() {
        let u = entry.value();
        let mfa_active = !u.mfa_devices.is_empty();
        let key1 = u.access_keys.first();
        let key2 = u.access_keys.get(1);

        let key1_active = key1.map(|k| k.status == "Active").unwrap_or(false);
        let key1_rotated = key1.map(|k| k.create_date.as_str()).unwrap_or("N/A");
        let key2_active = key2.map(|k| k.status == "Active").unwrap_or(false);
        let key2_rotated = key2.map(|k| k.create_date.as_str()).unwrap_or("N/A");

        csv.push_str(&format!(
            "{},{},{},true,N/A,N/A,N/A,{},{},{},,,,{},{},,,, false,N/A,false,N/A\n",
            u.user_name,
            u.arn,
            u.create_date,
            mfa_active,
            key1_active,
            key1_rotated,
            key2_active,
            key2_rotated,
        ));
    }

    // Base64-encode the CSV
    let encoded = base64_encode(csv.as_bytes());

    Ok(json!({
        "Content": encoded,
        "ReportFormat": "text/csv",
        "GeneratedTime": now_iso8601(),
    }))
}

/// Very simple base64 encoder (no external dependency).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };

        out.push(CHARS[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(CHARS[(((b0 & 0x3) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < data.len() {
            out.push(CHARS[(((b1 & 0xF) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(CHARS[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }
    out
}

pub fn generate_service_last_accessed_details(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    let job_id = new_uuid();
    Ok(json!({ "JobId": job_id }))
}

pub fn get_service_last_accessed_details(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let _job_id = require_str(input, "JobId")?;
    Ok(json!({
        "JobStatus": "COMPLETED",
        "JobCreationDate": now_iso8601(),
        "ServicesLastAccessed": { "member": [] },
        "IsTruncated": false,
    }))
}
