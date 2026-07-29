use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, Body, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;

use crate::state::{Message, SqsState};
use crate::util::{md5_of_message_attributes, queue_name_from_url};

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
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    // Expire inflight timeouts and re-queue them
    queue.tick();

    // FIFO ReceiveRequestAttemptId: AWS replays the original batch for
    // 5 minutes when the caller passes the same attempt id. Standard
    // queues silently ignore the parameter, so we only check it on
    // FIFO. Expire stale entries opportunistically so the cache stays
    // small without a sweeper.
    let attempt_id = input
        .get("ReceiveRequestAttemptId")
        .and_then(Value::as_str)
        .map(str::to_string);
    if queue.is_fifo {
        let now_inst = Instant::now();
        queue
            .receive_attempt_cache
            .retain(|_, (expiry, _)| *expiry > now_inst);
        if let Some(ref aid) = attempt_id
            && let Some((_, cached)) = queue.receive_attempt_cache.get(aid)
        {
            return Ok(cached.clone());
        }
    }

    // AWS caps inflight messages at 120000 for Standard queues and 20000
    // for FIFO queues; further Receive calls past the cap return
    // OverLimit so the producer can back off rather than silently piling
    // on more invisible messages.
    let inflight_cap = if queue.is_fifo { 20_000 } else { 120_000 };
    if queue.inflight.len() >= inflight_cap {
        return Err(AwsError::forbidden(
            "OverLimit",
            format!(
                "More than {inflight_cap} messages are inflight; \
                 delete or extend visibility before receiving more."
            ),
        ));
    }

    let now = Instant::now();

    // Determine visibility timeout for this receive call
    let visibility_timeout = input["VisibilityTimeout"]
        .as_u64()
        .unwrap_or_else(|| queue.visibility_timeout_secs());

    // Determine which attributes the caller wants. Per the SQS spec, omitting
    // AttributeNames / MessageAttributeNames returns no attributes. Only an
    // explicit ["All"] expands to every attribute.
    //
    // The 2019 API revision deprecated AttributeNames in favor of
    // MessageSystemAttributeNames; AWS still accepts both, with
    // MessageSystemAttributeNames taking precedence when both are present.
    let attribute_names: Vec<&str> = input["MessageSystemAttributeNames"]
        .as_array()
        .or_else(|| input["AttributeNames"].as_array())
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

        // Check if this message has exceeded maxReceiveCount. Route to DLQ
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

            // Update derived system attributes BEFORE collecting them for
            // the response so the caller sees the post-receive values:
            //   ApproximateReceiveCount. Incremented every receive
            //   ApproximateFirstReceiveTimestamp. Set once on first receive
            msg.attributes.insert(
                "ApproximateReceiveCount".to_string(),
                msg.receive_count.to_string(),
            );
            if msg.receive_count == 1 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                msg.attributes.insert(
                    "ApproximateFirstReceiveTimestamp".to_string(),
                    now_ms.to_string(),
                );
            }

            // Build attributes subset for the response
            let mut attrs = serde_json::Map::new();
            for (k, v) in &msg.attributes {
                if want_all_attrs || attribute_names.contains(&k.as_str()) {
                    attrs.insert(k.clone(), Value::String(v.clone()));
                }
            }

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

            // Always derive MD5OfMessageAttributes from the full attribute
            // set on the stored message. AWS sends it whenever the message
            // has any attributes, regardless of whether the caller asked
            // for them with MessageAttributeNames.
            if let Some(attr_md5) = md5_of_message_attributes(&msg.message_attributes) {
                msg_json["MD5OfMessageAttributes"] = Value::String(attr_md5);
            }

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

    let response = json!({ "Messages": messages_json });

    // Cache the response for the FIFO receive-idempotency window so a
    // network retry replays the same batch.
    //
    // Only non-empty batches are cached. Long polling calls this in a
    // loop, so memoising an empty batch would pin the caller to it for
    // the rest of the 5-minute window and defeat the wait entirely. An
    // empty batch has nothing to replay anyway.
    if !messages_json.is_empty()
        && let Some(aid) = attempt_id
        && let Some(mut q) = state.queues.get_mut(&queue_name)
        && q.is_fifo
    {
        let expiry = Instant::now() + std::time::Duration::from_secs(300);
        q.receive_attempt_cache
            .insert(aid, (expiry, response.clone()));
    }

    Ok(response)
}

/// Longest wait AWS permits on `ReceiveMessage`.
const MAX_WAIT_SECONDS: u64 = 20;

/// How often to re-check the queue while long polling.
///
/// A notification channel would wake instantly, but it would have to be
/// signalled from every path that can make a message visible: send,
/// visibility-timeout expiry, delay expiry, DLQ redrive, and the message
/// move tasks. Polling gets the same observable behaviour with one
/// mechanism instead of six, and 50 ms of latency is immaterial against
/// a wait measured in seconds.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Resolve the effective wait for this receive.
///
/// `WaitTimeSeconds` on the request wins; otherwise the queue's
/// `ReceiveMessageWaitTimeSeconds` attribute applies, which is how a
/// queue is configured for long polling by default.
fn resolve_wait_seconds(
    state: &SqsState,
    input: &Value,
    queue_name: &str,
) -> Result<u64, AwsError> {
    if let Some(raw) = input.get("WaitTimeSeconds") {
        let secs = raw.as_i64().ok_or_else(|| invalid_wait(raw))?;
        if !(0..=MAX_WAIT_SECONDS as i64).contains(&secs) {
            return Err(invalid_wait(raw));
        }
        return Ok(secs as u64);
    }

    let queue_default = state
        .queues
        .get(queue_name)
        .and_then(|q| {
            q.attributes
                .get("ReceiveMessageWaitTimeSeconds")
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0);
    Ok(queue_default.min(MAX_WAIT_SECONDS))
}

fn invalid_wait(raw: &Value) -> AwsError {
    AwsError::bad_request(
        "InvalidParameterValue",
        format!(
            "Value {raw} for parameter WaitTimeSeconds is invalid. \
             Reason: Must be >= 0 and <= {MAX_WAIT_SECONDS}."
        ),
    )
}

/// `ReceiveMessage` with long polling.
///
/// Blocks until a message is available or the wait elapses, returning as
/// soon as something arrives rather than always waiting the full period.
///
/// Without this the wait parameters were accepted and ignored, so a
/// consumer written against AWS long polling turned into a hot spin loop
/// against AWSim, and a test waiting on a producer either flaked or
/// passed for the wrong reason.
pub async fn handle_long_poll(
    state: &SqsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;
    let queue_name = queue_name_from_url(queue_url)?;
    let wait = resolve_wait_seconds(state, input, &queue_name)?;

    // Short-poll: one look, exactly as before.
    if wait == 0 {
        return handle(state, input, ctx);
    }

    let deadline = Instant::now() + Duration::from_secs(wait);
    loop {
        let response = handle(state, input, ctx)?;
        let empty = response["Messages"]
            .as_array()
            .is_none_or(|messages| messages.is_empty());
        if !empty || Instant::now() >= deadline {
            return Ok(response);
        }
        // Never overshoot the deadline waiting for the next look.
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(POLL_INTERVAL.min(remaining)).await;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::send_message;
    use crate::state::{Queue, SqsState};
    use std::collections::HashMap;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("sqs", "us-east-1")
    }

    fn standard_queue_state() -> SqsState {
        let state = SqsState::default();
        let q = Queue::new(
            "std".to_string(),
            "http://localhost/queue/std".to_string(),
            "arn:aws:sqs:us-east-1:000000000000:std".to_string(),
            false,
            "now".to_string(),
            HashMap::new(),
        );
        state.queues.insert("std".to_string(), q);
        state
    }

    #[test]
    fn message_system_attribute_names_takes_precedence_over_attribute_names() {
        let state = standard_queue_state();
        let ctx = ctx();
        send_message::handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
            }),
            &ctx,
        )
        .unwrap();

        // New parameter name (post-2019 API revision).
        let resp = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageSystemAttributeNames": ["All"],
            }),
            &ctx,
        )
        .unwrap();
        let msg = &resp["Messages"][0];
        let attrs = msg["Attributes"].as_object().expect("attributes returned");
        assert!(attrs.contains_key("ApproximateReceiveCount"));
    }

    #[test]
    fn first_receive_sets_approximate_first_receive_timestamp_and_count_one() {
        let state = standard_queue_state();
        let ctx = ctx();
        send_message::handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
            }),
            &ctx,
        )
        .unwrap();

        let resp = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "AttributeNames": ["All"],
            }),
            &ctx,
        )
        .unwrap();
        let msg = &resp["Messages"][0];
        let attrs = msg["Attributes"].as_object().expect("attributes returned");
        assert_eq!(
            attrs["ApproximateReceiveCount"].as_str(),
            Some("1"),
            "first receive must report count = 1"
        );
        let first_ts = attrs["ApproximateFirstReceiveTimestamp"]
            .as_str()
            .expect("ApproximateFirstReceiveTimestamp present");
        let parsed: u128 = first_ts.parse().expect("ms timestamp is numeric");
        assert!(parsed > 0, "ApproximateFirstReceiveTimestamp must be > 0");
    }

    fn fifo_state() -> SqsState {
        let state = SqsState::default();
        let mut attrs = HashMap::new();
        attrs.insert("FifoQueue".to_string(), "true".to_string());
        attrs.insert("ContentBasedDeduplication".to_string(), "true".to_string());
        let q = Queue::new(
            "q.fifo".to_string(),
            "http://localhost/queue/q.fifo".to_string(),
            "arn:aws:sqs:us-east-1:000000000000:q.fifo".to_string(),
            true,
            "now".to_string(),
            attrs,
        );
        state.queues.insert("q.fifo".to_string(), q);
        state
    }

    #[test]
    fn fifo_receive_request_attempt_id_replays_same_batch() {
        let state = fifo_state();
        let ctx = ctx();
        for n in 0..3 {
            send_message::handle(
                &state,
                &json!({
                    "QueueUrl": "http://localhost/queue/q.fifo",
                    "MessageBody": format!("msg-{n}"),
                    "MessageGroupId": "g",
                }),
                &ctx,
            )
            .unwrap();
        }

        // First receive with an attempt id consumes messages and caches
        // the response.
        let resp1 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MaxNumberOfMessages": 10,
                "ReceiveRequestAttemptId": "retry-1",
            }),
            &ctx,
        )
        .unwrap();
        let bodies1: Vec<String> = resp1["Messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["Body"].as_str().unwrap().to_string())
            .collect();
        let receipts1: Vec<String> = resp1["Messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["ReceiptHandle"].as_str().unwrap().to_string())
            .collect();

        // Second receive with the same attempt id replays bit-for-bit.
        let resp2 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MaxNumberOfMessages": 10,
                "ReceiveRequestAttemptId": "retry-1",
            }),
            &ctx,
        )
        .unwrap();
        let bodies2: Vec<String> = resp2["Messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["Body"].as_str().unwrap().to_string())
            .collect();
        let receipts2: Vec<String> = resp2["Messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["ReceiptHandle"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(bodies1, bodies2);
        assert_eq!(receipts1, receipts2);
    }
}
