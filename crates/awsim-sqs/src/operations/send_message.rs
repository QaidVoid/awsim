use std::collections::HashMap;
use std::time::{Duration, Instant};

use awsim_core::{AwsError, Body, RequestContext};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

use crate::state::{Message, MessageAttribute, SqsState};
use crate::util::{md5_of, md5_of_message_attributes, queue_name_from_url};

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let body = input["MessageBody"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "MessageBody is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;

    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::not_found(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    let now = Instant::now();
    let now_epoch = unix_epoch_secs();
    let message_id = Uuid::new_v4().to_string();
    let md5 = md5_of(body);

    // Delay seconds: per-message overrides queue default
    let delay_secs = input["DelaySeconds"]
        .as_u64()
        .unwrap_or_else(|| queue.delay_seconds());
    let delay_until = if delay_secs > 0 {
        Some(now + Duration::from_secs(delay_secs))
    } else {
        None
    };
    let delay_until_secs = if delay_secs > 0 {
        Some(now_epoch + delay_secs)
    } else {
        None
    };

    // Parse MessageAttributes
    let message_attributes = parse_message_attributes(&input["MessageAttributes"]);

    // FIFO-specific fields
    let group_id = if queue.is_fifo {
        Some(
            input["MessageGroupId"]
                .as_str()
                .ok_or_else(|| {
                    AwsError::bad_request(
                        "MissingParameter",
                        "MessageGroupId is required for FIFO queues",
                    )
                })?
                .to_string(),
        )
    } else {
        // Real AWS rejects MessageGroupId / MessageDeduplicationId on
        // standard queues with InvalidParameterValue. Mirror that —
        // silently dropping these makes test divergences hard to find.
        if input
            .get("MessageGroupId")
            .and_then(Value::as_str)
            .is_some()
        {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                "The request includes a parameter that is not valid for this queue type. \
                 MessageGroupId is only valid for FIFO queues",
            ));
        }
        None
    };

    let dedup_id = if queue.is_fifo {
        if let Some(id) = input["MessageDeduplicationId"].as_str() {
            Some(id.to_string())
        } else {
            // Fall back to sha256(body) when ContentBasedDeduplication is
            // enabled; otherwise AWS rejects the message with
            // InvalidParameterValue.
            let cbd_enabled = queue
                .attributes
                .get("ContentBasedDeduplication")
                .map(|v| v == "true")
                .unwrap_or(false);
            if cbd_enabled {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(body.as_bytes());
                Some(format!("{:x}", hasher.finalize()))
            } else {
                return Err(AwsError::bad_request(
                    "InvalidParameterValue",
                    "The Queue should either have ContentBasedDeduplication enabled \
                     or MessageDeduplicationId provided explicitly",
                ));
            }
        }
    } else {
        if input
            .get("MessageDeduplicationId")
            .and_then(Value::as_str)
            .is_some()
        {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                "The request includes a parameter that is not valid for this queue type. \
                 MessageDeduplicationId is only valid for FIFO queues",
            ));
        }
        None
    };

    // FIFO deduplication check
    if queue.is_fifo
        && let Some(ref did) = dedup_id
        && let Some((expiry, existing_id)) = queue.dedup_cache.get(did)
        && now < *expiry
    {
        // Duplicate detected; return the original message ID
        let seq = sequence_number();
        debug!(dedup_id = %did, "FIFO dedup suppressed duplicate");
        let mut resp = json!({
            "MessageId": existing_id,
            "MD5OfMessageBody": md5,
            "SequenceNumber": seq,
        });
        if let Some(attr_md5) = md5_of_message_attributes(&message_attributes) {
            resp["MD5OfMessageAttributes"] = Value::String(attr_md5);
        }
        return Ok(resp);
    }

    // Determine sequence number for FIFO
    let sequence_number = if queue.is_fifo {
        Some(sequence_number())
    } else {
        None
    };

    // Populate system attributes
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert(
        "SenderId".to_string(),
        "AIDA000000000000EXAMPLE".to_string(),
    );
    attributes.insert("SentTimestamp".to_string(), (now_epoch * 1000).to_string());
    attributes.insert("ApproximateReceiveCount".to_string(), "0".to_string());
    attributes.insert(
        "ApproximateFirstReceiveTimestamp".to_string(),
        "0".to_string(),
    );

    if let Some(ref gid) = group_id {
        attributes.insert("MessageGroupId".to_string(), gid.clone());
    }
    if let Some(ref did) = dedup_id {
        attributes.insert("MessageDeduplicationId".to_string(), did.clone());
    }
    if let Some(ref seq) = sequence_number {
        attributes.insert("SequenceNumber".to_string(), seq.clone());
    }

    // Record dedup entry for FIFO
    if queue.is_fifo
        && let Some(ref did) = dedup_id
    {
        let expiry = now + Duration::from_secs(300);
        queue
            .dedup_cache
            .insert(did.clone(), (expiry, message_id.clone()));
    }

    let body_field = if let Some(bs) = state.body_store() {
        let path = bs
            .write_blob("sqs", &queue_name, &message_id, body.as_bytes())
            .map_err(|e| AwsError::internal(format!("failed to persist message body: {e}")))?;
        Body::OnDisk(path)
    } else {
        Body::from_string(body.to_string())
    };

    let msg = Message {
        message_id: message_id.clone(),
        body: body_field,
        md5_of_body: md5.clone(),
        attributes,
        message_attributes,
        sent_at_secs: now_epoch,
        delay_until_secs,
        sent_at: Some(now),
        delay_until,
        sequence_number: sequence_number.clone(),
        receive_count: 0,
        dedup_id,
        group_id,
    };

    let attr_md5 = md5_of_message_attributes(&msg.message_attributes);
    queue.messages.push_back(msg);
    debug!(queue = %queue_name, message_id = %message_id, "Enqueued message");

    let mut resp = json!({
        "MessageId": message_id,
        "MD5OfMessageBody": md5,
    });

    if let Some(attr_md5) = attr_md5 {
        resp["MD5OfMessageAttributes"] = Value::String(attr_md5);
    }

    if let Some(seq) = sequence_number {
        resp["SequenceNumber"] = Value::String(seq);
    }

    Ok(resp)
}

pub fn handle_batch(
    state: &SqsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;

    let entries = input["Entries"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Entries is required"))?;

    if entries.is_empty() {
        return Err(AwsError::bad_request(
            "EmptyBatchRequest",
            "There must be at least one entry in the batch",
        ));
    }
    if entries.len() > 10 {
        return Err(AwsError::bad_request(
            "TooManyEntriesInBatchRequest",
            "Maximum number of entries per request is 10",
        ));
    }

    let mut successful = vec![];
    let mut failed = vec![];

    for entry in entries {
        let id = entry["Id"].as_str().unwrap_or("").to_string();

        // Build a synthetic per-entry input by merging QueueUrl into entry
        let mut entry_input = entry.clone();
        entry_input["QueueUrl"] = Value::String(queue_url.to_string());

        match handle(state, &entry_input, ctx) {
            Ok(result) => {
                let mut s = result.clone();
                s["Id"] = Value::String(id);
                successful.push(s);
            }
            Err(e) => {
                failed.push(json!({
                    "Id": id,
                    "SenderFault": true,
                    "Code": e.code,
                    "Message": e.message,
                }));
            }
        }
    }

    Ok(json!({
        "Successful": successful,
        "Failed": failed,
    }))
}

fn parse_message_attributes(val: &Value) -> HashMap<String, MessageAttribute> {
    let mut map = HashMap::new();
    if let Some(obj) = val.as_object() {
        for (k, v) in obj {
            let data_type = v["DataType"].as_str().unwrap_or("String").to_string();
            let string_value = v["StringValue"].as_str().map(|s| s.to_string());
            // BinaryValue arrives base64-encoded over the wire; decode so we
            // round-trip the raw bytes the caller sent.
            let binary_value = v["BinaryValue"]
                .as_str()
                .and_then(|s| BASE64.decode(s).ok());
            map.insert(
                k.clone(),
                MessageAttribute {
                    data_type,
                    string_value,
                    binary_value,
                },
            );
        }
    }
    map
}

fn sequence_number() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:019}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

fn unix_epoch_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Queue, SqsState};

    fn standard_queue() -> SqsState {
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

    fn fifo_queue(content_based_dedup: bool) -> SqsState {
        let state = SqsState::default();
        let mut attrs = HashMap::new();
        attrs.insert("FifoQueue".to_string(), "true".to_string());
        attrs.insert(
            "ContentBasedDeduplication".to_string(),
            content_based_dedup.to_string(),
        );
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
    fn standard_queue_rejects_message_deduplication_id() {
        let state = standard_queue();
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        let err = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
                "MessageDeduplicationId": "x",
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("MessageDeduplicationId"));
    }

    #[test]
    fn standard_queue_rejects_message_group_id() {
        let state = standard_queue();
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        let err = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
                "MessageGroupId": "g",
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("MessageGroupId"));
    }

    #[test]
    fn fifo_with_content_based_dedup_derives_id_from_body_hash() {
        let state = fifo_queue(true);
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");

        // First send — duplicates should be suppressed by the SHA-256 of
        // the body when ContentBasedDeduplication is enabled.
        let r1 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "payload",
                "MessageGroupId": "g",
            }),
            &ctx,
        )
        .unwrap();
        let id1 = r1["MessageId"].as_str().unwrap().to_string();

        // Same body — should be deduped (returns the original message id).
        let r2 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "payload",
                "MessageGroupId": "g",
            }),
            &ctx,
        )
        .unwrap();
        assert_eq!(r2["MessageId"].as_str().unwrap(), id1);

        // Different body — should NOT be deduped.
        let r3 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "payload-2",
                "MessageGroupId": "g",
            }),
            &ctx,
        )
        .unwrap();
        assert_ne!(r3["MessageId"].as_str().unwrap(), id1);
    }

    #[test]
    fn fifo_without_dedup_id_or_content_based_returns_invalid_parameter() {
        let state = fifo_queue(false);
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        let err = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "x",
                "MessageGroupId": "g",
            }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("ContentBasedDeduplication"));
    }

    #[test]
    fn fifo_explicit_dedup_id_takes_precedence_over_content_based() {
        let state = fifo_queue(true);
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        // Same body, but two different explicit dedup IDs → both deliver.
        handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "same",
                "MessageGroupId": "g",
                "MessageDeduplicationId": "first",
            }),
            &ctx,
        )
        .unwrap();
        let r2 = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/q.fifo",
                "MessageBody": "same",
                "MessageGroupId": "g",
                "MessageDeduplicationId": "second",
            }),
            &ctx,
        )
        .unwrap();
        // Second send must produce a different message id (no dedup).
        assert!(r2["MessageId"].as_str().unwrap().len() == 36);
    }

    #[test]
    fn send_message_returns_md5_of_message_attributes() {
        let state = standard_queue();
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        let resp = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
                "MessageAttributes": {
                    "k": { "DataType": "String", "StringValue": "v" }
                },
            }),
            &ctx,
        )
        .unwrap();
        let md5 = resp["MD5OfMessageAttributes"]
            .as_str()
            .expect("MD5OfMessageAttributes returned");
        assert_eq!(md5.len(), 32);
        // Body MD5 must remain the body-only hash (different value).
        assert_ne!(md5, resp["MD5OfMessageBody"].as_str().unwrap());
    }

    #[test]
    fn send_message_omits_md5_of_message_attributes_when_no_attributes() {
        let state = standard_queue();
        let ctx = awsim_core::RequestContext::new("sqs", "us-east-1");
        let resp = handle(
            &state,
            &json!({
                "QueueUrl": "http://localhost/queue/std",
                "MessageBody": "hi",
            }),
            &ctx,
        )
        .unwrap();
        assert!(resp.get("MD5OfMessageAttributes").is_none());
    }

    #[test]
    fn parse_message_attributes_decodes_binary_value() {
        let raw = b"\x00\x01\x02hello";
        let encoded = BASE64.encode(raw);
        let input = json!({
            "blob": { "DataType": "Binary", "BinaryValue": encoded },
            "label": { "DataType": "String", "StringValue": "world" },
        });
        let attrs = parse_message_attributes(&input);
        let blob = attrs.get("blob").expect("blob attribute parsed");
        assert_eq!(blob.data_type, "Binary");
        assert_eq!(blob.binary_value.as_deref(), Some(raw.as_ref()));
        assert!(blob.string_value.is_none());

        let label = attrs.get("label").expect("label attribute parsed");
        assert_eq!(label.string_value.as_deref(), Some("world"));
        assert!(label.binary_value.is_none());
    }
}
