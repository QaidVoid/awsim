use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::FirehoseState;

pub fn tag_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    if let Some(tags) = input["Tags"].as_array() {
        for t in tags {
            if let Some(k) = t["Key"].as_str() {
                let v = t["Value"].as_str().unwrap_or("").to_string();
                s.tags.insert(k.to_string(), v);
            }
        }
    }
    Ok(json!({}))
}

pub fn untag_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    validate_aws_tag_keys(&input["TagKeys"])?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    if let Some(keys) = input["TagKeys"].as_array() {
        for k in keys {
            if let Some(s_k) = k.as_str() {
                s.tags.remove(s_k);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags_for_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let s = state.streams.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    let tags: Vec<Value> = s
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();
    Ok(json!({
        "Tags": tags,
        "HasMoreTags": false,
    }))
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
                encryption_key_type: None,
                encryption_key_arn: None,
            },
        );
        state
    }

    #[test]
    fn tag_rejects_aws_prefix() {
        let state = state_with_stream("s1");
        let err = tag_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Tags": [{ "Key": "aws:internal", "Value": "v" }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation, got {err:?}",
        );
    }

    #[test]
    fn untag_rejects_aws_prefix() {
        let state = state_with_stream("s1");
        let err = untag_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "s1", "TagKeys": ["aws:internal"] }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation, got {err:?}",
        );
    }

    #[test]
    fn tag_rejects_out_of_charset_value() {
        let state = state_with_stream("s1");
        let err = tag_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Tags": [{ "Key": "env", "Value": "\u{0007}beep" }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(
            err.code.contains("Validation") || err.code.contains("InvalidParameter"),
            "expected validation, got {err:?}",
        );
    }

    #[test]
    fn tag_persists_well_formed_tags_then_untag_removes_them() {
        let state = state_with_stream("s1");
        tag_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Tags": [
                    { "Key": "env", "Value": "prod" },
                    { "Key": "team", "Value": "data" },
                ],
            }),
            &ctx(),
        )
        .unwrap();
        let listed =
            list_tags_for_delivery_stream(&state, &json!({ "DeliveryStreamName": "s1" }), &ctx())
                .unwrap();
        assert_eq!(listed["Tags"].as_array().unwrap().len(), 2);

        untag_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "s1", "TagKeys": ["env"] }),
            &ctx(),
        )
        .unwrap();
        let listed =
            list_tags_for_delivery_stream(&state, &json!({ "DeliveryStreamName": "s1" }), &ctx())
                .unwrap();
        let tags = listed["Tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["Key"], "team");
    }
}
