use std::collections::HashMap;

use awsim_core::{AwsError, InternalEvent, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::filter;
use crate::state::{MessageAttribute, PublishedMessage, SnsState};

// ---------------------------------------------------------------------------
// Publish
// ---------------------------------------------------------------------------

pub fn publish(state: &SnsState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    // TopicArn or TargetArn
    let topic_arn = input["TopicArn"]
        .as_str()
        .or_else(|| input["TargetArn"].as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "TopicArn or TargetArn is required")
        })?;

    let message = input["Message"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Message is required"))?;

    let topic = state
        .topics
        .get(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    if topic.is_fifo {
        if input["MessageGroupId"].as_str().is_none() {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "MessageGroupId is required for FIFO topics.",
            ));
        }
        let has_dedup_id = input["MessageDeduplicationId"].as_str().is_some();
        let cbd = topic
            .attributes
            .get("ContentBasedDeduplication")
            .map(|v| v == "true")
            .unwrap_or(false);
        if !has_dedup_id && !cbd {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "MessageDeduplicationId is required for FIFO topics unless ContentBasedDeduplication is enabled.",
            ));
        }
    }
    drop(topic);

    // When MessageStructure="json", the Message must parse as a JSON object
    // with at least a "default" key — AWS rejects otherwise. We keep the
    // raw payload here and pick the per-protocol body during fan-out via
    // select_message_for_protocol().
    let message_structure = input["MessageStructure"].as_str();
    let message_json: Option<serde_json::Map<String, Value>> = match message_structure {
        Some("json") => {
            let parsed: Value = serde_json::from_str(message).map_err(|_| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "Message must be valid JSON when MessageStructure is 'json'",
                )
            })?;
            let obj = parsed.as_object().cloned().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "MessageStructure='json' requires Message to be a JSON object",
                )
            })?;
            if !obj.contains_key("default") {
                return Err(AwsError::bad_request(
                    "InvalidParameter",
                    "Attribute 'default' is required when MessageStructure is 'json'",
                ));
            }
            Some(obj)
        }
        Some(other) => {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                format!("Unknown MessageStructure: {other}"),
            ));
        }
        None => None,
    };

    let subject = input["Subject"].as_str().map(str::to_string);
    let message_attributes = parse_message_attributes(input)?;

    let payload_bytes = message.len() + attributes_payload_size(&message_attributes);
    if payload_bytes > SNS_MAX_PAYLOAD_BYTES {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!(
                "Message + MessageAttributes payload is {payload_bytes} bytes; \
                 SNS rejects payloads larger than {SNS_MAX_PAYLOAD_BYTES} bytes."
            ),
        ));
    }

    let message_id = Uuid::new_v4().to_string();

    let published = PublishedMessage {
        message_id: message_id.clone(),
        topic_arn: topic_arn.to_string(),
        message: message.to_string(),
        subject: subject.clone(),
        message_attributes,
    };

    info!(
        message_id = %message_id,
        topic = %topic_arn,
        subject = ?subject,
        "Published message"
    );

    fan_out_to_subscribers(
        state,
        ctx,
        topic_arn,
        &message_id,
        message,
        message_json.as_ref(),
        subject.as_deref(),
        &published.message_attributes,
    );

    let _ = published;

    Ok(json!({ "MessageId": message_id }))
}

/// Emit cross-service `sns:Publish` events for every subscription on
/// `topic_arn` that targets SQS or Lambda, applying the subscription's
/// FilterPolicy and per-protocol body selection. Used by both Publish
/// and PublishBatch so batch entries fan out to subscribers identically.
#[allow(clippy::too_many_arguments)]
fn fan_out_to_subscribers(
    state: &SnsState,
    ctx: &RequestContext,
    topic_arn: &str,
    message_id: &str,
    raw_message: &str,
    message_json: Option<&serde_json::Map<String, Value>>,
    subject: Option<&str>,
    message_attributes: &HashMap<String, MessageAttribute>,
) {
    let Some(bus) = ctx.event_bus.as_ref() else {
        return;
    };

    let filter_attrs: HashMap<String, Value> = message_attributes
        .iter()
        .map(|(k, attr)| {
            let val = json!({
                "DataType": attr.data_type,
                "Value": attr.string_value.as_deref().unwrap_or(""),
            });
            (k.clone(), val)
        })
        .collect();

    let attr_envelope: serde_json::Map<String, Value> = message_attributes
        .iter()
        .map(|(k, attr)| {
            let entry = json!({
                "Type": attr.data_type,
                "Value": attr.string_value.as_deref().unwrap_or(""),
            });
            (k.clone(), entry)
        })
        .collect();

    let subs: Vec<(String, String, String, Option<String>, String, bool)> = state
        .subscriptions
        .iter()
        .filter(|s| s.topic_arn == topic_arn && (s.protocol == "sqs" || s.protocol == "lambda"))
        .map(|s| {
            let filter_policy = s.attributes.get("FilterPolicy").cloned();
            let scope = s
                .attributes
                .get("FilterPolicyScope")
                .cloned()
                .unwrap_or_else(|| "MessageAttributes".to_string());
            let raw_delivery = s
                .attributes
                .get("RawMessageDelivery")
                .map(|v| v == "true")
                .unwrap_or(false);
            (
                s.protocol.clone(),
                s.endpoint.clone(),
                s.arn.clone(),
                filter_policy,
                scope,
                raw_delivery,
            )
        })
        .collect();

    // Lazily parse the message body — only when at least one subscription
    // uses FilterPolicyScope=MessageBody, since most don't.
    let body_value: Option<Value> = if subs
        .iter()
        .any(|(_, _, _, _, scope, _)| scope == "MessageBody")
    {
        serde_json::from_str(raw_message).ok()
    } else {
        None
    };

    // Delivery-status feedback is configured per-protocol at the topic
    // level. When the matching *SuccessFeedbackRoleArn /
    // *FailureFeedbackRoleArn is set, real SNS emits AWS/SNS CloudWatch
    // metrics on each delivery. Thread the gate flags + topic name into
    // the event so the router can emit without re-reading SNS state.
    let topic_attrs: HashMap<String, String> = state
        .topics
        .get(topic_arn)
        .map(|t| t.attributes.clone())
        .unwrap_or_default();
    let topic_name = topic_arn.rsplit(':').next().unwrap_or("").to_string();

    for (protocol, endpoint, subscription_arn, filter_policy, scope, raw_delivery) in subs {
        if let Some(filter_str) = &filter_policy
            && let Ok(filter_val) = serde_json::from_str::<Value>(filter_str)
        {
            let passes = match scope.as_str() {
                "MessageBody" => match &body_value {
                    Some(body) => filter::matches_filter_body(&filter_val, body),
                    None => false, // Body not parseable as JSON → can't match.
                },
                _ => filter::matches_filter(&filter_val, &filter_attrs),
            };
            if !passes {
                continue;
            }
        }

        let delivered = select_message_for_protocol(message_json, &protocol).unwrap_or(raw_message);

        let (succ_attr, fail_attr) = match protocol.as_str() {
            "sqs" => ("SQSSuccessFeedbackRoleArn", "SQSFailureFeedbackRoleArn"),
            "lambda" => (
                "LambdaSuccessFeedbackRoleArn",
                "LambdaFailureFeedbackRoleArn",
            ),
            _ => ("", ""),
        };
        let success_feedback = topic_attrs.get(succ_attr).is_some_and(|s| !s.is_empty());
        let failure_feedback = topic_attrs.get(fail_attr).is_some_and(|s| !s.is_empty());

        let event = InternalEvent {
            source: "sns".to_string(),
            event_type: "sns:Publish".to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "topic_arn": topic_arn,
                "message_id": message_id,
                "message": delivered,
                "subject": subject,
                "protocol": protocol,
                "endpoint": endpoint,
                "subscription_arn": subscription_arn,
                "message_attributes": attr_envelope,
                "raw_message_delivery": raw_delivery,
                "topic_name": topic_name.clone(),
                "success_feedback": success_feedback,
                "failure_feedback": failure_feedback,
            }),
        };
        bus.publish(event);
    }
}

// ---------------------------------------------------------------------------
// PublishBatch
// ---------------------------------------------------------------------------

pub fn publish_batch(
    state: &SnsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    if !state.topics.contains_key(topic_arn) {
        return Err(AwsError::not_found(
            "NotFound",
            format!("Topic not found: {topic_arn}"),
        ));
    }

    let entries = input["PublishBatchRequestEntries"]
        .as_array()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "PublishBatchRequestEntries is required")
        })?;

    if entries.is_empty() {
        return Err(AwsError::bad_request(
            "EmptyBatchRequest",
            "PublishBatchRequestEntries must not be empty",
        ));
    }
    if entries.len() > 10 {
        return Err(AwsError::bad_request(
            "TooManyEntriesInBatchRequest",
            "Maximum 10 entries per batch",
        ));
    }

    // AWS rejects the whole batch with BatchEntryIdsNotDistinct when
    // two entries share an Id. Validate up front so callers can fix
    // the duplicate before any side effects.
    //
    // Each Id must also satisfy the AWS-documented format: 1..=80
    // characters from `[A-Za-z0-9_-]`. Missing / malformed Ids surface
    // as InvalidBatchEntryId so SDKs can distinguish "wrong shape"
    // from "duplicate".
    let mut seen_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for entry in entries {
        let id = entry["Id"].as_str().unwrap_or("");
        if id.is_empty()
            || id.len() > 80
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            return Err(AwsError::bad_request(
                "InvalidBatchEntryId",
                format!("Batch entry Id `{id}` must be 1..=80 characters of [A-Za-z0-9_-]."),
            ));
        }
        if !seen_ids.insert(id) {
            return Err(AwsError::bad_request(
                "BatchEntryIdsNotDistinct",
                format!("Id `{id}` repeats in the batch request."),
            ));
        }
    }

    // AWS caps total PublishBatch payload at 256 KB; per-entry caps
    // are enforced separately inside the loop after attributes parse.
    let total_bytes: usize = entries
        .iter()
        .filter_map(|e| e["Message"].as_str())
        .map(str::len)
        .sum();
    if total_bytes > SNS_MAX_PAYLOAD_BYTES {
        return Err(AwsError::bad_request(
            "BatchRequestTooLong",
            format!(
                "Batch request total size {total_bytes} bytes exceeds the {SNS_MAX_PAYLOAD_BYTES}-byte limit."
            ),
        ));
    }

    // FIFO topics enable per-entry dedup. ContentBasedDeduplication
    // hashes the Message payload; an explicit MessageDeduplicationId
    // wins over CBD when both are present.
    let topic_is_fifo = state
        .topics
        .get(topic_arn)
        .map(|t| t.is_fifo)
        .unwrap_or(false);
    let cbd_enabled = state
        .topics
        .get(topic_arn)
        .and_then(|t| {
            t.attributes
                .get("ContentBasedDeduplication")
                .map(|v| v == "true")
        })
        .unwrap_or(false);

    let mut successful: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();

    for entry in entries {
        let id = entry["Id"].as_str().unwrap_or("").to_string();
        let message = entry["Message"].as_str();

        match message {
            None => {
                failed.push(json!({
                    "Id": id,
                    "Code": "InvalidParameter",
                    "Message": "Message is required",
                    "SenderFault": true,
                }));
            }
            Some(msg) => {
                let dedup_key = if topic_is_fifo {
                    fifo_dedup_key(entry, msg, cbd_enabled)
                } else {
                    None
                };
                if let Some(ref key) = dedup_key
                    && let Some(existing) = lookup_fifo_dedup(state, topic_arn, key)
                {
                    successful.push(json!({
                        "Id": id,
                        "MessageId": existing,
                    }));
                    continue;
                }
                let message_id = Uuid::new_v4().to_string();
                let subject = entry["Subject"].as_str().map(str::to_string);
                let message_attributes = match parse_message_attributes(entry) {
                    Ok(m) => m,
                    Err(e) => {
                        failed.push(json!({
                            "Id": id,
                            "Code": e.code,
                            "Message": e.message,
                            "SenderFault": true,
                        }));
                        continue;
                    }
                };

                let entry_bytes = msg.len() + attributes_payload_size(&message_attributes);
                if entry_bytes > SNS_MAX_PAYLOAD_BYTES {
                    failed.push(json!({
                        "Id": id,
                        "Code": "InvalidParameterValue",
                        "Message": format!(
                            "Entry payload is {entry_bytes} bytes; SNS rejects entries larger than {SNS_MAX_PAYLOAD_BYTES} bytes."
                        ),
                        "SenderFault": true,
                    }));
                    continue;
                }

                info!(
                    message_id = %message_id,
                    topic = %topic_arn,
                    batch_id = %id,
                    "Published batch message"
                );

                let published = PublishedMessage {
                    message_id: message_id.clone(),
                    topic_arn: topic_arn.to_string(),
                    message: msg.to_string(),
                    subject: subject.clone(),
                    message_attributes: message_attributes.clone(),
                };

                fan_out_to_subscribers(
                    state,
                    ctx,
                    topic_arn,
                    &message_id,
                    msg,
                    None, // PublishBatch doesn't accept MessageStructure
                    subject.as_deref(),
                    &message_attributes,
                );

                let _ = published;

                if let Some(key) = dedup_key {
                    record_fifo_dedup(state, topic_arn, &key, &message_id);
                }

                successful.push(json!({
                    "Id": id,
                    "MessageId": message_id,
                }));
            }
        }
    }

    Ok(json!({
        "Successful": successful,
        "Failed": failed,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const SNS_MAX_PAYLOAD_BYTES: usize = 262_144;

fn attributes_payload_size(attrs: &HashMap<String, MessageAttribute>) -> usize {
    attrs
        .iter()
        .map(|(name, attr)| {
            let value_bytes = attr
                .string_value
                .as_ref()
                .map(|s| s.len())
                .or_else(|| attr.binary_value.as_ref().map(|b| b.len()))
                .unwrap_or(0);
            name.len() + attr.data_type.len() + value_bytes
        })
        .sum()
}

/// When `MessageStructure="json"` was supplied on Publish, select the
/// protocol-specific message body from the parsed JSON. Returns `None`
/// when no MessageStructure was provided (caller should deliver the raw
/// message). Falls back to the "default" key when the protocol-specific
/// key is absent.
fn select_message_for_protocol<'a>(
    message_json: Option<&'a serde_json::Map<String, Value>>,
    protocol: &str,
) -> Option<&'a str> {
    let obj = message_json?;
    obj.get(protocol)
        .or_else(|| obj.get("default"))
        .and_then(Value::as_str)
}

fn parse_message_attributes(input: &Value) -> Result<HashMap<String, MessageAttribute>, AwsError> {
    let mut result = HashMap::new();
    let Some(attrs) = input["MessageAttributes"].as_object() else {
        return Ok(result);
    };
    for (name, attr) in attrs {
        let data_type = attr["DataType"].as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValue",
                format!("MessageAttribute `{name}` is missing DataType."),
            )
        })?;
        let base = data_type.split('.').next().unwrap_or("");
        if !matches!(base, "String" | "Number" | "Binary") {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!(
                    "MessageAttribute `{name}` has unsupported DataType `{data_type}`; \
                     must be String / Number / Binary, optionally with a .CustomType suffix."
                ),
            ));
        }
        let string_value = attr["StringValue"].as_str().map(str::to_string);
        let binary_value = match attr["BinaryValue"].as_str() {
            Some(s) => Some(BASE64.decode(s).map_err(|_| {
                AwsError::bad_request(
                    "InvalidParameterValue",
                    format!("MessageAttribute `{name}` BinaryValue is not valid base64."),
                )
            })?),
            None => None,
        };
        match base {
            "String" | "Number" if string_value.is_none() => {
                return Err(AwsError::bad_request(
                    "InvalidParameterValue",
                    format!(
                        "MessageAttribute `{name}` is `{base}` but no StringValue was supplied."
                    ),
                ));
            }
            "Binary" if binary_value.is_none() => {
                return Err(AwsError::bad_request(
                    "InvalidParameterValue",
                    format!("MessageAttribute `{name}` is Binary but no BinaryValue was supplied."),
                ));
            }
            _ => {}
        }
        result.insert(
            name.clone(),
            MessageAttribute {
                data_type: data_type.to_string(),
                string_value,
                binary_value,
            },
        );
    }
    Ok(result)
}

/// Compute the FIFO dedup key for a single batch entry. Returns `None`
/// when neither an explicit `MessageDeduplicationId` nor
/// content-based dedup applies — the caller should treat the entry as
/// non-deduplicating.
fn fifo_dedup_key(entry: &Value, message: &str, cbd_enabled: bool) -> Option<String> {
    if let Some(id) = entry["MessageDeduplicationId"].as_str()
        && !id.is_empty()
    {
        return Some(format!("id:{id}"));
    }
    if cbd_enabled {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(message.as_bytes());
        return Some(format!("sha256:{:x}", h.finalize()));
    }
    None
}

const FIFO_DEDUP_TTL_SECS: u64 = 300;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Look up a recent dedup decision, evicting expired entries on the way
/// past. Returns the original MessageId when the entry is still within
/// the 5-minute window.
fn lookup_fifo_dedup(state: &SnsState, topic_arn: &str, key: &str) -> Option<String> {
    let cache_key = format!("{topic_arn}|{key}");
    let entry = state.fifo_dedup_cache.get(&cache_key)?.clone();
    let now = now_secs();
    if now.saturating_sub(entry.1) > FIFO_DEDUP_TTL_SECS {
        state.fifo_dedup_cache.remove(&cache_key);
        return None;
    }
    Some(entry.0)
}

fn record_fifo_dedup(state: &SnsState, topic_arn: &str, key: &str, message_id: &str) {
    let cache_key = format!("{topic_arn}|{key}");
    state
        .fifo_dedup_cache
        .insert(cache_key, (message_id.to_string(), now_secs()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wrap(attr: Value) -> Value {
        json!({ "MessageAttributes": { "k": attr } })
    }

    #[test]
    fn accepts_string_attribute() {
        let v = wrap(json!({ "DataType": "String", "StringValue": "hello" }));
        let out = parse_message_attributes(&v).unwrap();
        let a = &out["k"];
        assert_eq!(a.data_type, "String");
        assert_eq!(a.string_value.as_deref(), Some("hello"));
        assert!(a.binary_value.is_none());
    }

    #[test]
    fn accepts_custom_string_suffix() {
        let v = wrap(json!({ "DataType": "String.Phone", "StringValue": "+1" }));
        let out = parse_message_attributes(&v).unwrap();
        assert_eq!(out["k"].data_type, "String.Phone");
    }

    #[test]
    fn decodes_binary_value() {
        let v = wrap(json!({ "DataType": "Binary", "BinaryValue": BASE64.encode(b"abc") }));
        let out = parse_message_attributes(&v).unwrap();
        assert_eq!(out["k"].binary_value.as_deref(), Some(&b"abc"[..]));
    }

    #[test]
    fn rejects_missing_data_type() {
        let v = wrap(json!({ "StringValue": "x" }));
        let err = parse_message_attributes(&v).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("missing DataType"));
    }

    #[test]
    fn rejects_unknown_data_type() {
        let v = wrap(json!({ "DataType": "Object", "StringValue": "x" }));
        let err = parse_message_attributes(&v).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("unsupported DataType"));
    }

    #[test]
    fn rejects_string_without_string_value() {
        let v = wrap(json!({ "DataType": "String" }));
        let err = parse_message_attributes(&v).unwrap_err();
        assert!(err.message.contains("no StringValue was supplied"));
    }

    #[test]
    fn rejects_binary_without_binary_value() {
        let v = wrap(json!({ "DataType": "Binary" }));
        let err = parse_message_attributes(&v).unwrap_err();
        assert!(err.message.contains("no BinaryValue was supplied"));
    }

    #[test]
    fn rejects_malformed_base64() {
        let v = wrap(json!({ "DataType": "Binary", "BinaryValue": "!!!not-base64!!!" }));
        let err = parse_message_attributes(&v).unwrap_err();
        assert!(err.message.contains("not valid base64"));
    }

    fn ctx_us() -> RequestContext {
        RequestContext::new("sns", "us-east-1")
    }

    fn make_fifo_topic(state: &SnsState, arn: &str, cbd: bool) {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            "ContentBasedDeduplication".to_string(),
            if cbd { "true" } else { "false" }.to_string(),
        );
        attrs.insert("FifoTopic".to_string(), "true".to_string());
        state.topics.insert(
            arn.to_string(),
            crate::state::Topic {
                arn: arn.to_string(),
                name: arn.rsplit(':').next().unwrap().to_string(),
                attributes: attrs,
                tags: Default::default(),
                is_fifo: true,
                subscription_arns: Vec::new(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
        );
    }

    fn make_topic_with_attrs(state: &SnsState, arn: &str, attrs: Vec<(&str, &str)>) {
        let mut map = std::collections::HashMap::new();
        for (k, v) in attrs {
            map.insert(k.to_string(), v.to_string());
        }
        state.topics.insert(
            arn.to_string(),
            crate::state::Topic {
                arn: arn.to_string(),
                name: arn.rsplit(':').next().unwrap().to_string(),
                attributes: map,
                tags: Default::default(),
                is_fifo: false,
                subscription_arns: Vec::new(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
        );
    }

    fn subscribe_sqs(state: &SnsState, topic: &str) {
        let arn = format!("{topic}:sub-1");
        state.subscriptions.insert(
            arn.clone(),
            crate::state::Subscription {
                arn,
                topic_arn: topic.to_string(),
                protocol: "sqs".to_string(),
                endpoint: "arn:aws:sqs:us-east-1:000000000000:q".to_string(),
                confirmed: true,
                attributes: Default::default(),
            },
        );
    }

    fn ctx_with_bus() -> (RequestContext, awsim_core::events::EventBus) {
        let bus = awsim_core::events::EventBus::new();
        let mut ctx = RequestContext::new("sns", "us-east-1");
        ctx.event_bus = Some(bus.clone());
        (ctx, bus)
    }

    #[test]
    fn fan_out_event_carries_success_feedback_gate() {
        let state = SnsState::default();
        let topic = "arn:aws:sns:us-east-1:000000000000:t";
        make_topic_with_attrs(
            &state,
            topic,
            vec![(
                "SQSSuccessFeedbackRoleArn",
                "arn:aws:iam::000000000000:role/r",
            )],
        );
        subscribe_sqs(&state, topic);
        let (ctx, bus) = ctx_with_bus();
        let mut rx = bus.subscribe();
        publish(&state, &json!({ "TopicArn": topic, "Message": "hi" }), &ctx).unwrap();
        let ev = rx.try_recv().expect("expected one sns:Publish event");
        assert_eq!(ev.detail["protocol"], "sqs");
        assert_eq!(ev.detail["topic_name"], "t");
        assert_eq!(ev.detail["success_feedback"], true);
        assert_eq!(ev.detail["failure_feedback"], false);
    }

    #[test]
    fn fan_out_event_has_no_feedback_gate_without_role() {
        let state = SnsState::default();
        let topic = "arn:aws:sns:us-east-1:000000000000:t";
        make_topic_with_attrs(&state, topic, vec![]);
        subscribe_sqs(&state, topic);
        let (ctx, bus) = ctx_with_bus();
        let mut rx = bus.subscribe();
        publish(&state, &json!({ "TopicArn": topic, "Message": "hi" }), &ctx).unwrap();
        let ev = rx.try_recv().expect("expected one sns:Publish event");
        assert_eq!(ev.detail["success_feedback"], false);
        assert_eq!(ev.detail["failure_feedback"], false);
    }

    #[test]
    fn publish_batch_reuses_message_id_for_explicit_dedup_id() {
        let state = SnsState::default();
        let topic = "arn:aws:sns:us-east-1:000000000000:orders.fifo";
        make_fifo_topic(&state, topic, false);
        let resp1 = publish_batch(
            &state,
            &json!({
                "TopicArn": topic,
                "PublishBatchRequestEntries": [
                    {
                        "Id": "e1",
                        "Message": "first",
                        "MessageGroupId": "g1",
                        "MessageDeduplicationId": "DUP",
                    }
                ]
            }),
            &ctx_us(),
        )
        .unwrap();
        let first_id = resp1["Successful"][0]["MessageId"]
            .as_str()
            .unwrap()
            .to_string();
        let resp2 = publish_batch(
            &state,
            &json!({
                "TopicArn": topic,
                "PublishBatchRequestEntries": [
                    {
                        "Id": "e2",
                        "Message": "different body",
                        "MessageGroupId": "g1",
                        "MessageDeduplicationId": "DUP",
                    }
                ]
            }),
            &ctx_us(),
        )
        .unwrap();
        let second_id = resp2["Successful"][0]["MessageId"].as_str().unwrap();
        assert_eq!(second_id, first_id);
    }

    #[test]
    fn publish_batch_content_based_dedup_matches_identical_payload() {
        let state = SnsState::default();
        let topic = "arn:aws:sns:us-east-1:000000000000:orders.fifo";
        make_fifo_topic(&state, topic, true);
        let resp = publish_batch(
            &state,
            &json!({
                "TopicArn": topic,
                "PublishBatchRequestEntries": [
                    {"Id": "a", "Message": "same", "MessageGroupId": "g1"},
                    {"Id": "b", "Message": "same", "MessageGroupId": "g1"},
                    {"Id": "c", "Message": "different", "MessageGroupId": "g1"}
                ]
            }),
            &ctx_us(),
        )
        .unwrap();
        let successes = resp["Successful"].as_array().unwrap();
        assert_eq!(successes.len(), 3);
        assert_eq!(successes[0]["MessageId"], successes[1]["MessageId"]);
        assert_ne!(successes[0]["MessageId"], successes[2]["MessageId"]);
    }

    #[test]
    fn publish_batch_does_not_dedup_on_non_fifo_topic() {
        let state = SnsState::default();
        let topic = "arn:aws:sns:us-east-1:000000000000:standard";
        state.topics.insert(
            topic.to_string(),
            crate::state::Topic {
                arn: topic.to_string(),
                name: "standard".into(),
                attributes: Default::default(),
                tags: Default::default(),
                is_fifo: false,
                subscription_arns: Vec::new(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
        );
        let resp = publish_batch(
            &state,
            &json!({
                "TopicArn": topic,
                "PublishBatchRequestEntries": [
                    {"Id": "a", "Message": "same"},
                    {"Id": "b", "Message": "same"}
                ]
            }),
            &ctx_us(),
        )
        .unwrap();
        let successes = resp["Successful"].as_array().unwrap();
        assert_ne!(successes[0]["MessageId"], successes[1]["MessageId"]);
    }
}
