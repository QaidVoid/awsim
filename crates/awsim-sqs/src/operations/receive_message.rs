use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, Body, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;

use crate::state::{Message, SqsState};
use crate::util::queue_name_from_url;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let max_messages = input["MaxNumberOfMessages"]
        .as_u64()
        .unwrap_or(1)
        .clamp(1, 10) as usize;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    // Expire inflight timeouts and re-queue them
    queue.tick();

    let now = Instant::now();

    // Determine visibility timeout for this receive call
    let visibility_timeout = input["VisibilityTimeout"]
        .as_u64()
        .unwrap_or_else(|| queue.visibility_timeout_secs());

    // Determine which attributes the caller wants. Per the SQS spec, omitting
    // AttributeNames / MessageAttributeNames returns no attributes — only an
    // explicit ["All"] expands to every attribute.
    let attribute_names: Vec<&str> = input["AttributeNames"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let want_all_attrs = attribute_names.contains(&"All");

    let message_attribute_names: Vec<&str> = input["MessageAttributeNames"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let want_all_msg_attrs = message_attribute_names.contains(&"All");

    // Snapshot the redrive policy so we can release the queue borrow later.
    let redrive_policy = queue.redrive_policy.clone();

    let mut messages_json = vec![];
    let mut to_inflight: Vec<String> = vec![];
    let mut dlq_messages: Vec<Message> = vec![];

    // Collect up to max_messages visible messages
    for msg in queue.messages.iter() {
        if to_inflight.len() + dlq_messages.len() >= max_messages {
            break;
        }
        // Skip delayed messages
        if msg.delay_until.is_some_and(|d| d > now) {
            continue;
        }

        // Check if this message has exceeded maxReceiveCount — route to DLQ
        if let Some(ref rp) = redrive_policy
            && msg.receive_count >= rp.max_receive_count
        {
            dlq_messages.push(msg.clone());
            continue;
        }

        to_inflight.push(msg.message_id.clone());
    }

    // Remove DLQ-bound messages from main queue first
    for dlq_msg in &dlq_messages {
        if let Some(pos) = queue
            .messages
            .iter()
            .position(|m| m.message_id == dlq_msg.message_id)
        {
            queue.messages.remove(pos);
        }
    }

    // Move selected messages to inflight
    for msg_id in &to_inflight {
        // Find the message in the deque and remove it
        if let Some(pos) = queue.messages.iter().position(|m| &m.message_id == msg_id)
            && let Some(mut msg) = queue.messages.remove(pos)
        {
            let receipt_handle = Uuid::new_v4().to_string();
            let visible_at = now + Duration::from_secs(visibility_timeout);
            let now_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let visible_at_secs = now_epoch + visibility_timeout;

            // Increment receive_count now
            msg.receive_count += 1;

            // Build attributes subset
            let mut attrs = serde_json::Map::new();
            for (k, v) in &msg.attributes {
                if want_all_attrs || attribute_names.contains(&k.as_str()) {
                    attrs.insert(k.clone(), Value::String(v.clone()));
                }
            }
            // Update receive count in the attribute map for the response
            msg.attributes.insert(
                "ApproximateReceiveCount".to_string(),
                msg.receive_count.to_string(),
            );

            // Build message attributes subset
            let mut msg_attrs = serde_json::Map::new();
            if want_all_msg_attrs {
                for (k, ma) in &msg.message_attributes {
                    msg_attrs.insert(k.clone(), Value::Object(message_attribute_entry(ma)));
                }
            } else {
                for name in &message_attribute_names {
                    if let Some(ma) = msg.message_attributes.get(*name) {
                        msg_attrs
                            .insert(name.to_string(), Value::Object(message_attribute_entry(ma)));
                    }
                }
            }

            let body_str = msg
                .body
                .read_string()
                .map_err(|e| AwsError::internal(format!("failed to read message body: {e}")))?;

            let mut msg_json = json!({
                "MessageId": msg.message_id,
                "ReceiptHandle": receipt_handle,
                "Body": body_str,
                "MD5OfBody": msg.md5_of_body,
            });

            if !attrs.is_empty() {
                msg_json["Attributes"] = Value::Object(attrs);
            }
            if !msg_attrs.is_empty() {
                msg_json["MessageAttributes"] = Value::Object(msg_attrs);
            }

            messages_json.push(msg_json);

            // Move to inflight
            let im = crate::state::InflightMessage {
                message: msg,
                visible_at: Some(visible_at),
                visible_at_secs,
                receipt_handle: receipt_handle.clone(),
            };
            queue.inflight.insert(receipt_handle, im);
        }
    }

    // Release the queue borrow before writing to DLQ (avoids deadlock on DashMap)
    drop(queue);

    // Move DLQ-bound messages to the dead-letter queue
    if !dlq_messages.is_empty()
        && let Some(ref rp) = redrive_policy
        && let Some(dlq_name) = state.queue_name_by_arn(&rp.dead_letter_target_arn)
        && let Some(mut dlq) = state.queues.get_mut(&dlq_name)
    {
        for mut msg in dlq_messages {
            if let (Body::OnDisk(_), Some(bs)) = (&msg.body, state.body_store()) {
                match msg.body.read_string() {
                    Ok(bytes) => {
                        match bs.write_blob("sqs", &dlq_name, &msg.message_id, bytes.as_bytes()) {
                            Ok(new_path) => {
                                if let Err(e) = bs.delete_blob("sqs", &queue_name, &msg.message_id)
                                {
                                    warn!(
                                        queue = %queue_name,
                                        message_id = %msg.message_id,
                                        error = %e,
                                        "Failed to delete source blob after DLQ migration",
                                    );
                                }
                                msg.body = Body::OnDisk(new_path);
                            }
                            Err(e) => {
                                warn!(
                                    dlq = %dlq_name,
                                    message_id = %msg.message_id,
                                    error = %e,
                                    "Failed to write DLQ blob; falling back to in-memory body",
                                );
                                msg.body = Body::from_string(bytes);
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            queue = %queue_name,
                            message_id = %msg.message_id,
                            error = %e,
                            "Failed to read message body during DLQ migration",
                        );
                    }
                }
            }
            dlq.messages.push_back(msg);
        }
    }

    Ok(json!({ "Messages": messages_json }))
}

fn message_attribute_entry(ma: &crate::state::MessageAttribute) -> serde_json::Map<String, Value> {
    let mut entry = serde_json::Map::new();
    entry.insert("DataType".to_string(), Value::String(ma.data_type.clone()));
    if let Some(ref sv) = ma.string_value {
        entry.insert("StringValue".to_string(), Value::String(sv.clone()));
    }
    if let Some(ref bv) = ma.binary_value {
        entry.insert("BinaryValue".to_string(), Value::String(BASE64.encode(bv)));
    }
    entry
}
