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
    // AWS documents 22 columns for the credential report CSV. Use a
    // typed row builder so missing values can't sneak in (or get
    // accidentally indented as part of a wider string literal) and
    // every row is comma-separated cleanly.
    const COLUMNS: &[&str] = &[
        "user",
        "arn",
        "user_creation_time",
        "password_enabled",
        "password_last_used",
        "password_last_changed",
        "password_next_rotation",
        "mfa_active",
        "access_key_1_active",
        "access_key_1_last_rotated",
        "access_key_1_last_used_date",
        "access_key_1_last_used_region",
        "access_key_1_last_used_service",
        "access_key_2_active",
        "access_key_2_last_rotated",
        "access_key_2_last_used_date",
        "access_key_2_last_used_region",
        "access_key_2_last_used_service",
        "cert_1_active",
        "cert_1_last_rotated",
        "cert_2_active",
        "cert_2_last_rotated",
    ];

    fn row(cells: &[&str]) -> String {
        debug_assert_eq!(cells.len(), 22, "credential_report row must have 22 cells");
        cells.join(",")
    }

    let mut lines: Vec<String> = Vec::with_capacity(1 + state.users.len() + 1);
    lines.push(COLUMNS.join(","));

    // Root account: AWS uses `not_supported` for fields that aren't
    // applicable to the root user (e.g. password_last_used predates the
    // rollout of last-used tracking).
    lines.push(row(&[
        "<root_account>",
        "arn:aws:iam::root",
        "",
        "not_supported",
        "not_supported",
        "not_supported",
        "not_supported",
        "false",
        "false",
        "N/A",
        "N/A",
        "N/A",
        "N/A",
        "false",
        "N/A",
        "N/A",
        "N/A",
        "N/A",
        "false",
        "N/A",
        "false",
        "N/A",
    ]));

    for entry in state.users.iter() {
        let u = entry.value();
        let mfa_active = if !u.mfa_devices.is_empty() {
            "true"
        } else {
            "false"
        };
        let key1 = u.access_keys.first();
        let key2 = u.access_keys.get(1);
        let key1_active = key1
            .map(|k| {
                if k.status == "Active" {
                    "true"
                } else {
                    "false"
                }
            })
            .unwrap_or("false");
        let key1_rotated = key1.map(|k| k.create_date.as_str()).unwrap_or("N/A");
        let key2_active = key2
            .map(|k| {
                if k.status == "Active" {
                    "true"
                } else {
                    "false"
                }
            })
            .unwrap_or("false");
        let key2_rotated = key2.map(|k| k.create_date.as_str()).unwrap_or("N/A");

        lines.push(row(&[
            &u.user_name,
            &u.arn,
            &u.create_date,
            "true",
            "N/A",
            "N/A",
            "N/A",
            mfa_active,
            key1_active,
            key1_rotated,
            "N/A",
            "N/A",
            "N/A",
            key2_active,
            key2_rotated,
            "N/A",
            "N/A",
            "N/A",
            "false",
            "N/A",
            "false",
            "N/A",
        ]));
    }

    // Trailing newline matches what real AWS returns.
    let mut csv = lines.join("\n");
    csv.push('\n');

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

#[cfg(test)]
mod credential_report_tests {
    use super::*;
    use crate::state::IamState;

    fn decode(b64: &str) -> String {
        // Inverse of `base64_encode`; only used for tests so a brittle
        // implementation is fine.
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn header_lists_all_22_aws_columns() {
        let state = IamState::default();
        let resp = get_credential_report(&state, &Value::Null).unwrap();
        let csv = decode(resp["Content"].as_str().unwrap());
        let header = csv.lines().next().unwrap();
        assert_eq!(header.split(',').count(), 22);
        assert!(header.starts_with("user,arn,user_creation_time"));
    }

    #[test]
    fn root_row_has_22_fields() {
        let state = IamState::default();
        let resp = get_credential_report(&state, &Value::Null).unwrap();
        let csv = decode(resp["Content"].as_str().unwrap());
        let root = csv.lines().nth(1).unwrap();
        assert_eq!(root.split(',').count(), 22);
        assert!(root.starts_with("<root_account>,arn:aws:iam::root,"));
        assert!(!root.contains(" "));
    }

    #[test]
    fn user_row_does_not_have_extra_spaces() {
        let state = IamState::default();
        crate::operations::users::create_user(
            &state,
            &serde_json::json!({ "UserName": "alice" }),
            &awsim_core::RequestContext::new("iam", "us-east-1"),
        )
        .unwrap();
        let resp = get_credential_report(&state, &Value::Null).unwrap();
        let csv = decode(resp["Content"].as_str().unwrap());
        let user_row = csv.lines().nth(2).expect("user row present");
        assert_eq!(user_row.split(',').count(), 22);
        assert!(
            !user_row.contains(" "),
            "stray whitespace in user row: {user_row}"
        );
    }
}
