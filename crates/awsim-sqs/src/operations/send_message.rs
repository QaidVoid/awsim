use std::collections::HashMap;
use std::time::{Duration, Instant};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

use crate::state::{Message, MessageAttribute, SqsState};
use crate::util::{md5_of, queue_name_from_url};

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
        None
    };

    let dedup_id = if queue.is_fifo {
        input["MessageDeduplicationId"]
            .as_str()
            .map(|s| s.to_string())
    } else {
        None
    };

    // FIFO deduplication check
    if queue.is_fifo
        && let Some(ref did) = dedup_id
            && let Some((expiry, existing_id)) = queue.dedup_cache.get(did)
                && now < *expiry {
                    // Duplicate detected; return the original message ID
                    let seq = sequence_number();
                    debug!(dedup_id = %did, "FIFO dedup suppressed duplicate");
                    return Ok(json!({
                        "MessageId": existing_id,
                        "MD5OfMessageBody": md5,
                        "SequenceNumber": seq,
                    }));
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
        && let Some(ref did) = dedup_id {
            let expiry = now + Duration::from_secs(300);
            queue
                .dedup_cache
                .insert(did.clone(), (expiry, message_id.clone()));
        }

    let msg = Message {
        message_id: message_id.clone(),
        body: body.to_string(),
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

    queue.messages.push_back(msg);
    debug!(queue = %queue_name, message_id = %message_id, "Enqueued message");

    let mut resp = json!({
        "MessageId": message_id,
        "MD5OfMessageBody": md5,
    });

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
            map.insert(
                k.clone(),
                MessageAttribute {
                    data_type,
                    string_value,
                    binary_value: None,
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
