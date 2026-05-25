use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{KinesisState, StreamConsumer, now_secs};

// ---------------------------------------------------------------------------
// RegisterStreamConsumer
// ---------------------------------------------------------------------------

pub fn register_stream_consumer(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_arn = input["StreamARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamARN is required"))?;
    let consumer_name = input["ConsumerName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ConsumerName is required"))?;

    // Verify the stream exists
    let stream_name = stream_arn.rsplit('/').next().unwrap_or("");
    if !state.streams.contains_key(stream_name) {
        return Err(AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream ARN {} does not exist", stream_arn),
        ));
    }

    let consumer_arn = format!(
        "{}/consumer/{}:{}",
        stream_arn,
        consumer_name,
        uuid::Uuid::new_v4()
    );

    let consumer = StreamConsumer {
        consumer_arn: consumer_arn.clone(),
        consumer_name: consumer_name.to_string(),
        consumer_status: "ACTIVE".to_string(),
        stream_arn: stream_arn.to_string(),
        consumer_creation_timestamp: now_secs(),
    };

    state
        .consumers
        .insert(consumer_arn.clone(), consumer.clone());

    Ok(json!({
        "Consumer": {
            "ConsumerARN": consumer.consumer_arn,
            "ConsumerName": consumer.consumer_name,
            "ConsumerStatus": consumer.consumer_status,
            "ConsumerCreationTimestamp": consumer.consumer_creation_timestamp,
        }
    }))
}

// ---------------------------------------------------------------------------
// DeregisterStreamConsumer
// ---------------------------------------------------------------------------

pub fn deregister_stream_consumer(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let consumer_arn = input["ConsumerARN"].as_str();
    let stream_arn = input["StreamARN"].as_str();
    let consumer_name = input["ConsumerName"].as_str();

    // Resolve by ConsumerARN (preferred) or by stream+name
    if let Some(arn) = consumer_arn {
        state.consumers.remove(arn);
        return Ok(json!({}));
    }

    if let (Some(s_arn), Some(name)) = (stream_arn, consumer_name) {
        state
            .consumers
            .retain(|_, c| !(c.stream_arn == s_arn && c.consumer_name == name));
        return Ok(json!({}));
    }

    Err(AwsError::bad_request(
        "InvalidParameter",
        "Either ConsumerARN or both StreamARN and ConsumerName are required",
    ))
}

// ---------------------------------------------------------------------------
// DescribeStreamConsumer
// ---------------------------------------------------------------------------

pub fn describe_stream_consumer(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let consumer_arn = input["ConsumerARN"].as_str();
    let stream_arn = input["StreamARN"].as_str();
    let consumer_name = input["ConsumerName"].as_str();

    let consumer = if let Some(arn) = consumer_arn {
        state
            .consumers
            .get(arn)
            .ok_or_else(|| {
                AwsError::bad_request(
                    "ResourceNotFoundException",
                    format!("Consumer not found: {arn}"),
                )
            })?
            .clone()
    } else if let (Some(s_arn), Some(name)) = (stream_arn, consumer_name) {
        state
            .consumers
            .iter()
            .find(|e| e.stream_arn == s_arn && e.consumer_name == name)
            .map(|e| e.value().clone())
            .ok_or_else(|| {
                AwsError::bad_request(
                    "ResourceNotFoundException",
                    format!("Consumer '{}' not found on stream {}", name, s_arn),
                )
            })?
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Either ConsumerARN or both StreamARN and ConsumerName are required",
        ));
    };

    Ok(json!({
        "ConsumerDescription": {
            "ConsumerARN": consumer.consumer_arn,
            "ConsumerName": consumer.consumer_name,
            "ConsumerStatus": consumer.consumer_status,
            "ConsumerCreationTimestamp": consumer.consumer_creation_timestamp,
            "StreamARN": consumer.stream_arn,
        }
    }))
}

// ---------------------------------------------------------------------------
// ListStreamConsumers
// ---------------------------------------------------------------------------

pub fn list_stream_consumers(
    state: &KinesisState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let stream_arn = input["StreamARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "StreamARN is required"))?;

    let consumers: Vec<Value> = state
        .consumers
        .iter()
        .filter(|e| e.value().stream_arn == stream_arn)
        .map(|e| {
            let c = e.value();
            json!({
                "ConsumerARN": c.consumer_arn,
                "ConsumerName": c.consumer_name,
                "ConsumerStatus": c.consumer_status,
                "ConsumerCreationTimestamp": c.consumer_creation_timestamp,
            })
        })
        .collect();

    Ok(json!({ "Consumers": consumers }))
}

// ---------------------------------------------------------------------------
// SubscribeToShard (stub)
// ---------------------------------------------------------------------------

pub fn subscribe_to_shard(
    _state: &KinesisState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // Full streaming not supported in this architecture.
    // Return a stub empty event stream response.
    Ok(json!({ "EventStream": [] }))
}
