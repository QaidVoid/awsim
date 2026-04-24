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

    // Build a Value-based view of message attributes for filter evaluation.
    let filter_attrs: HashMap<String, Value> = published
        .message_attributes
        .iter()
        .map(|(k, attr)| {
            let val = json!({
                "DataType": attr.data_type,
                "Value": attr.string_value.as_deref().unwrap_or(""),
            });
            (k.clone(), val)
        })
        .collect();

    // Emit cross-service events for each active subscription.
    if let Some(bus) = ctx.event_bus.as_ref() {
        // Collect subscriptions for this topic that target SQS or Lambda.
        let subs: Vec<(String, String, Option<String>)> = state
            .subscriptions
            .iter()
            .filter(|s| s.topic_arn == topic_arn && (s.protocol == "sqs" || s.protocol == "lambda"))
            .map(|s| {
                let filter_policy = s.attributes.get("FilterPolicy").cloned();
                (s.protocol.clone(), s.endpoint.clone(), filter_policy)
            })
            .collect();

        for (protocol, endpoint, filter_policy) in subs {
            // Apply filter policy if set
            if let Some(filter_str) = &filter_policy
                && let Ok(filter_val) = serde_json::from_str::<Value>(filter_str)
                    && !filter::matches_filter(&filter_val, &filter_attrs) {
                        continue; // Skip this subscription
                    }

            let event = InternalEvent {
                source: "sns".to_string(),
                event_type: "sns:Publish".to_string(),
                region: ctx.region.clone(),
                account_id: ctx.account_id.clone(),
                detail: json!({
                    "topic_arn": topic_arn,
                    "message_id": message_id,
                    "message": message,
                    "subject": subject,
                    "protocol": protocol,
                    "endpoint": endpoint,
                }),
            };
            bus.publish(event);
        }
    }

    let _ = published;

    Ok(json!({ "MessageId": message_id }))
}

// ---------------------------------------------------------------------------
// PublishBatch
// ---------------------------------------------------------------------------

pub fn publish_batch(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
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
                    subject,
                    message_attributes,
                };
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
