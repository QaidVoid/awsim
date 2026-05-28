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

/// AWS Firehose caps `ListTagsForDeliveryStream` results at 50 per
/// page. Callers paginate via `ExclusiveStartTagKey` (the last tag
/// key returned in the previous page) and watch `HasMoreTags` to
/// know when to stop.
const LIST_TAGS_MAX_PAGE: usize = 50;

pub fn list_tags_for_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let limit = match input.get("Limit").and_then(Value::as_i64) {
        Some(n) if !(1..=LIST_TAGS_MAX_PAGE as i64).contains(&n) => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("Limit `{n}` must be in 1..={LIST_TAGS_MAX_PAGE}."),
            ));
        }
        Some(n) => n as usize,
        None => LIST_TAGS_MAX_PAGE,
    };
    let start_after = input
        .get("ExclusiveStartTagKey")
        .and_then(Value::as_str)
        .map(String::from);

    let s = state.streams.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    // Stable ordering by tag key so pagination cursor advances
    // deterministically across calls.
    let mut keys: Vec<&String> = s.tags.keys().collect();
    keys.sort();
    let starting_idx = match start_after {
        Some(k) => keys.iter().position(|tk| **tk > k).unwrap_or(keys.len()),
        None => 0,
    };
    let end_idx = (starting_idx + limit).min(keys.len());
    let page = &keys[starting_idx..end_idx];
    let tags: Vec<Value> = page
        .iter()
        .map(|k| json!({ "Key": k, "Value": s.tags.get(*k).cloned().unwrap_or_default() }))
        .collect();
    Ok(json!({
        "Tags": tags,
        "HasMoreTags": end_idx < keys.len(),
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
                source_config: None,
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
    fn list_tags_paginates_with_limit_and_has_more_tags() {
        let state = state_with_stream("s1");
        let pairs: Vec<(String, String)> = (0..7)
            .map(|i| (format!("k{i:02}"), format!("v{i}")))
            .collect();
        tag_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Tags": pairs.iter().map(|(k, v)| json!({ "Key": k, "Value": v })).collect::<Vec<_>>(),
            }),
            &ctx(),
        )
        .unwrap();

        let page1 = list_tags_for_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "s1", "Limit": 3 }),
            &ctx(),
        )
        .unwrap();
        let p1_tags: Vec<&str> = page1["Tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["Key"].as_str().unwrap())
            .collect();
        assert_eq!(p1_tags, &["k00", "k01", "k02"]);
        assert_eq!(page1["HasMoreTags"], true);

        let page2 = list_tags_for_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Limit": 3,
                "ExclusiveStartTagKey": "k02",
            }),
            &ctx(),
        )
        .unwrap();
        let p2_tags: Vec<&str> = page2["Tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["Key"].as_str().unwrap())
            .collect();
        assert_eq!(p2_tags, &["k03", "k04", "k05"]);
        assert_eq!(page2["HasMoreTags"], true);

        let page3 = list_tags_for_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "s1",
                "Limit": 3,
                "ExclusiveStartTagKey": "k05",
            }),
            &ctx(),
        )
        .unwrap();
        let p3_tags: Vec<&str> = page3["Tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["Key"].as_str().unwrap())
            .collect();
        assert_eq!(p3_tags, &["k06"]);
        assert_eq!(page3["HasMoreTags"], false);
    }

    #[test]
    fn list_tags_rejects_out_of_range_limit() {
        let state = state_with_stream("s1");
        let err = list_tags_for_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "s1", "Limit": 0 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
        let err = list_tags_for_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "s1", "Limit": 51 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
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
