use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::FirehoseState;

pub fn start_encryption(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let key_type = input["DeliveryStreamEncryptionConfigurationInput"]["KeyType"]
        .as_str()
        .map(String::from);
    let key_arn = input["DeliveryStreamEncryptionConfigurationInput"]["KeyARN"]
        .as_str()
        .map(String::from);
    // CUSTOMER_MANAGED_CMK requires a real KMS key ARN. AWS_OWNED_CMK
    // (the default) draws from the AWS-owned pool, so callers must
    // not pass a `KeyARN` with it.
    match key_type.as_deref() {
        Some("CUSTOMER_MANAGED_CMK") => {
            let arn = key_arn.as_deref().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidArgumentException",
                    "KeyARN is required when KeyType is CUSTOMER_MANAGED_CMK.",
                )
            })?;
            validate_kms_key_arn(arn)?;
        }
        Some("AWS_OWNED_CMK") | None => {
            if key_arn.is_some() {
                return Err(AwsError::bad_request(
                    "InvalidArgumentException",
                    "KeyARN must be omitted when KeyType is AWS_OWNED_CMK.",
                ));
            }
        }
        Some(other) => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("KeyType `{other}` must be AWS_OWNED_CMK or CUSTOMER_MANAGED_CMK."),
            ));
        }
    }

    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    // Encryption is asynchronous: the stream enters ENABLING now and the
    // tick driver promotes it to ENABLED (arming the Encrypted flag).
    s.encryption_status = "ENABLING".to_string();
    s.encryption_key_type = key_type;
    s.encryption_key_arn = key_arn;
    Ok(json!({}))
}

/// Validate a KMS key ARN of the form
/// `arn:<partition>:kms:<region>:<account>:key/<uuid>` or
/// `.../alias/<name>`. Length and shape come from AWS docs; we
/// accept both partition values so callers running against
/// `aws-cn` or `aws-us-gov` stay compatible.
fn validate_kms_key_arn(arn: &str) -> Result<(), AwsError> {
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    let shape_ok = parts.len() == 6
        && parts[0] == "arn"
        && !parts[1].is_empty()
        && parts[2] == "kms"
        && !parts[3].is_empty()
        && parts[4].len() == 12
        && parts[4].chars().all(|c| c.is_ascii_digit())
        && (parts[5].starts_with("key/") || parts[5].starts_with("alias/"))
        && parts[5].len() > "key/".len();
    if !shape_ok {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("KeyARN `{arn}` is not a valid KMS key ARN."),
        ));
    }
    Ok(())
}

pub fn stop_encryption(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    // Only ENABLED / ENABLING streams can be stopped; otherwise this is a
    // no-op transition that AWS rejects.
    if !matches!(s.encryption_status.as_str(), "ENABLED" | "ENABLING") {
        return Err(AwsError::bad_request(
            "ResourceInUseException",
            "Encryption is not enabled on this delivery stream.",
        ));
    }
    s.encryption_status = "DISABLING".to_string();
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DeliveryStream;
    use std::collections::HashMap;

    fn ctx() -> RequestContext {
        RequestContext::new("firehose", "us-east-1")
    }

    fn state_with_stream(name: &str) -> FirehoseState {
        let state = FirehoseState::default();
        state.streams.insert(
            name.into(),
            DeliveryStream {
                name: name.into(),
                arn: format!("arn:aws:firehose:us-east-1:123456789012:deliverystream/{name}"),
                status: "ACTIVE".into(),
                stream_type: "DirectPut".into(),
                version_id: "1".into(),
                create_timestamp: 0,
                last_update_timestamp: 0,
                destinations: Vec::new(),
                has_more_destinations: false,
                tags: HashMap::new(),
                encryption_enabled: false,
                encryption_status: "DISABLED".into(),
                encryption_key_type: None,
                encryption_key_arn: None,
                source_config: None,
            },
        );
        state
    }

    #[test]
    fn start_encryption_requires_key_arn_for_cmk() {
        let state = state_with_stream("s1");
        let err = start_encryption(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "DeliveryStreamEncryptionConfigurationInput": { "KeyType": "CUSTOMER_MANAGED_CMK" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
        assert!(err.message.contains("KeyARN"));
    }

    #[test]
    fn start_encryption_rejects_bad_key_arn() {
        let state = state_with_stream("s1");
        for bad in [
            "not-an-arn",
            "arn:aws:s3:::my-bucket",
            "arn:aws:kms:us-east-1:1234:key/abc",
            "arn:aws:kms:us-east-1:123456789012:notakey/abc",
        ] {
            let err = start_encryption(
                &state,
                &json!({
                    "DeliveryStreamName": "s1",
                    "DeliveryStreamEncryptionConfigurationInput": {
                        "KeyType": "CUSTOMER_MANAGED_CMK",
                        "KeyARN": bad,
                    },
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "InvalidArgumentException", "input {bad}");
        }
    }

    #[test]
    fn start_encryption_accepts_well_formed_cmk() {
        let state = state_with_stream("s1");
        start_encryption(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "DeliveryStreamEncryptionConfigurationInput": {
                    "KeyType": "CUSTOMER_MANAGED_CMK",
                    "KeyARN": "arn:aws:kms:us-east-1:123456789012:key/abcdef01-2345-6789-abcd-ef0123456789",
                },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn encryption_walks_enabling_then_enabled_on_advance() {
        let state = state_with_stream("s1");
        start_encryption(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "DeliveryStreamEncryptionConfigurationInput": { "KeyType": "AWS_OWNED_CMK" },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            state.streams.get("s1").unwrap().encryption_status,
            "ENABLING"
        );
        assert!(!state.streams.get("s1").unwrap().encryption_enabled);
        state.streams.get_mut("s1").unwrap().advance_encryption();
        assert_eq!(
            state.streams.get("s1").unwrap().encryption_status,
            "ENABLED"
        );
        assert!(state.streams.get("s1").unwrap().encryption_enabled);
    }

    #[test]
    fn stop_encryption_walks_disabling_then_disabled_and_clears_keys() {
        let state = state_with_stream("s1");
        start_encryption(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "DeliveryStreamEncryptionConfigurationInput": {
                    "KeyType": "CUSTOMER_MANAGED_CMK",
                    "KeyARN": "arn:aws:kms:us-east-1:123456789012:key/abcdef01-2345-6789-abcd-ef0123456789",
                },
            }),
            &ctx(),
        )
        .unwrap();
        state.streams.get_mut("s1").unwrap().advance_encryption(); // -> ENABLED
        stop_encryption(&state, &json!({ "DeliveryStreamName": "s1" }), &ctx()).unwrap();
        assert_eq!(
            state.streams.get("s1").unwrap().encryption_status,
            "DISABLING"
        );
        state.streams.get_mut("s1").unwrap().advance_encryption(); // -> DISABLED
        let s = state.streams.get("s1").unwrap();
        assert_eq!(s.encryption_status, "DISABLED");
        assert!(!s.encryption_enabled);
        assert!(s.encryption_key_arn.is_none());
    }

    #[test]
    fn stop_encryption_rejects_when_already_disabled() {
        let state = state_with_stream("s1");
        let err =
            stop_encryption(&state, &json!({ "DeliveryStreamName": "s1" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "ResourceInUseException");
    }

    #[test]
    fn start_encryption_rejects_arn_with_aws_owned() {
        let state = state_with_stream("s1");
        let err = start_encryption(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "DeliveryStreamEncryptionConfigurationInput": {
                    "KeyType": "AWS_OWNED_CMK",
                    "KeyARN": "arn:aws:kms:us-east-1:123456789012:key/abc",
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }
}
