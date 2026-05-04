use std::collections::HashMap;

use awsim_core::{AwsError, InternalEvent, RequestContext};
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

    if !state.topics.contains_key(topic_arn) {
        return Err(AwsError::not_found(
            "NotFound",
            format!("Topic not found: {topic_arn}"),
        ));
    }

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
    let message_attributes = parse_message_attributes(input);

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

    let subs: Vec<(String, String, String, Option<String>, bool)> = state
        .subscriptions
        .iter()
        .filter(|s| s.topic_arn == topic_arn && (s.protocol == "sqs" || s.protocol == "lambda"))
        .map(|s| {
            let filter_policy = s.attributes.get("FilterPolicy").cloned();
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
                raw_delivery,
            )
        })
        .collect();

    for (protocol, endpoint, subscription_arn, filter_policy, raw_delivery) in subs {
        if let Some(filter_str) = &filter_policy
            && let Ok(filter_val) = serde_json::from_str::<Value>(filter_str)
            && !filter::matches_filter(&filter_val, &filter_attrs)
        {
            continue;
        }

        let delivered = select_message_for_protocol(message_json, &protocol).unwrap_or(raw_message);

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
            "InvalidParameter",
            "PublishBatchRequestEntries must not be empty",
        ));
    }
    if entries.len() > 10 {
        return Err(AwsError::bad_request(
            "TooManyEntriesInBatchRequest",
            "Maximum 10 entries per batch",
        ));
    }

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
                let message_id = Uuid::new_v4().to_string();
                let subject = entry["Subject"].as_str().map(str::to_string);
                let message_attributes = parse_message_attributes(entry);

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

fn parse_message_attributes(input: &Value) -> HashMap<String, MessageAttribute> {
    let mut result = HashMap::new();
    if let Some(attrs) = input["MessageAttributes"].as_object() {
        for (name, attr) in attrs {
            let data_type = attr["DataType"].as_str().unwrap_or("String").to_string();
            let string_value = attr["StringValue"].as_str().map(str::to_string);
            result.insert(
                name.clone(),
                MessageAttribute {
                    data_type,
                    string_value,
                    binary_value: None,
                },
            );
        }
    }
    result
}
