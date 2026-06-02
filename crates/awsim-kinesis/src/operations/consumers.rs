use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{CONSUMER_IDLE_SECS, KinesisState, StreamConsumer, now_secs};

/// Deregister enhanced-fan-out consumers idle past
/// [`CONSUMER_IDLE_SECS`], measured against `now` (Unix seconds).
/// Returns the number of consumers removed. Absolute-time gated and
/// idempotent, so the tick loop can call it on every pass.
pub fn sweep_idle_consumers(state: &KinesisState, now: u64) -> usize {
    let stale: Vec<String> = state
        .consumers
        .iter()
        .filter(|e| now.saturating_sub(e.value().last_active_secs) > CONSUMER_IDLE_SECS)
        .map(|e| e.key().clone())
        .collect();
    for arn in &stale {
        state.consumers.remove(arn);
    }
    stale.len()
}

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

    let now = now_secs();
    let consumer = StreamConsumer {
        consumer_arn: consumer_arn.clone(),
        consumer_name: consumer_name.to_string(),
        consumer_status: "ACTIVE".to_string(),
        stream_arn: stream_arn.to_string(),
        consumer_creation_timestamp: now,
        last_active_secs: now,
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
// SubscribeToShard (enhanced fan-out)
// ---------------------------------------------------------------------------

/// Highest sequence number streamed per `SubscribeToShard` response.
/// Real EFO pushes records continuously for up to 5 minutes; we emit a
/// single finite buffered batch of frames so the request completes.
const SUBSCRIBE_BATCH_LIMIT: usize = 10_000;

/// SubscribeToShard — push records for one shard to an enhanced
/// fan-out consumer. The architecture is request/response, so we emit
/// a finite buffered batch of event-stream frames carrying the records
/// that match `StartingPosition`. The frames use the protocol-layer
/// `__awsim_eventstream__` marker shape so the gateway encodes them as
/// `application/vnd.amazon.eventstream` binary frames.
pub fn subscribe_to_shard(
    state: &KinesisState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let consumer_arn = input["ConsumerARN"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ConsumerARN is required"))?;
    let shard_id = input["ShardId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "ShardId is required"))?;

    let consumer = state
        .consumers
        .get(consumer_arn)
        .ok_or_else(|| {
            AwsError::bad_request(
                "ResourceNotFoundException",
                format!("Consumer not found: {consumer_arn}"),
            )
        })?
        .value()
        .clone();

    let stream_name = consumer.stream_arn.rsplit('/').next().unwrap_or("");
    let stream = state.streams.get(stream_name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {stream_name} does not exist"),
        )
    })?;

    if !stream.shards.iter().any(|s| s.shard_id == shard_id) {
        return Err(AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Shard {shard_id} does not exist in stream {stream_name}"),
        ));
    }
    drop(stream);

    let sqlite = state
        .sqlite()
        .ok_or_else(|| AwsError::internal("Kinesis sqlite store not initialised"))?;

    // Derive the exclusive lower-bound cursor from StartingPosition,
    // mirroring GetShardIterator's iterator-type semantics.
    let position = starting_position(
        &input["StartingPosition"],
        sqlite,
        &ctx.account_id,
        &ctx.region,
        stream_name,
        shard_id,
    )?;

    let rows = sqlite.read_after(
        &ctx.account_id,
        &ctx.region,
        stream_name,
        shard_id,
        position as i64,
        SUBSCRIBE_BATCH_LIMIT,
    )?;

    let records: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "SequenceNumber": format!("{:020}", r.seq),
                "ApproximateArrivalTimestamp": r.timestamp_millis / 1000,
                "Data": r.data,
                "PartitionKey": r.partition_key,
            })
        })
        .collect();

    let last_seq = rows.last().map(|r| r.seq as u64).unwrap_or(position);
    let continuation = format!("{last_seq:020}");

    // Refresh the consumer's activity clock so the idle sweep keeps it.
    if let Some(mut c) = state.consumers.get_mut(consumer_arn) {
        c.last_active_secs = now_secs();
    }

    // Single SubscribeToShardEvent frame using the protocol-layer
    // marker shape (awsim-core/src/protocol/eventstream.rs::MARKER).
    let frame = json!({
        "headers": {
            ":event-type": "SubscribeToShardEvent",
            ":content-type": "application/json",
            ":message-type": "event",
        },
        "payload": {
            "Records": records,
            "ContinuationSequenceNumber": continuation,
            "MillisBehindLatest": 0,
            "ChildShards": [],
        }
    });

    Ok(json!({ "__awsim_eventstream__": [frame] }))
}

/// Resolve a `StartingPosition` object into an exclusive sequence-
/// number cursor (`seq > position` is streamed). Mirrors the iterator
/// semantics in `get_shard_iterator::handle`.
fn starting_position(
    pos: &Value,
    sqlite: &crate::SqliteStore,
    account: &str,
    region: &str,
    stream_name: &str,
    shard_id: &str,
) -> Result<u64, AwsError> {
    let kind = pos["Type"].as_str().unwrap_or("TRIM_HORIZON");
    Ok(match kind {
        "TRIM_HORIZON" | "AT_TIMESTAMP" => 0,
        "LATEST" => sqlite
            .max_seq(account, region, stream_name, shard_id)
            .unwrap_or(0) as u64,
        "AT_SEQUENCE_NUMBER" => {
            let seq: u64 = pos["SequenceNumber"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            // Inclusive: step back one so the named record is read next.
            seq.saturating_sub(1)
        }
        "AFTER_SEQUENCE_NUMBER" => pos["SequenceNumber"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        other => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("Unknown StartingPosition.Type: {other}"),
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::SqliteStore;
    use crate::state::{KinesisStream, Shard};

    fn ctx() -> RequestContext {
        RequestContext::new("000000000000", "us-east-1")
    }

    fn store_with_sqlite() -> KinesisState {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-kinesis-consumer-test-{id}.db"));
        let state = KinesisState::default();
        state.set_sqlite(Arc::new(SqliteStore::open(path).unwrap()));
        state
    }

    fn seed_stream(state: &KinesisState, name: &str) {
        let stream = KinesisStream {
            name: name.to_string(),
            arn: format!("arn:aws:kinesis:us-east-1:000000000000:stream/{name}"),
            status: "ACTIVE".to_string(),
            shards: vec![Shard::new_range(0, 0, u128::MAX)],
            retention_hours: 24,
            tags: Default::default(),
            created_at: 0,
            enhanced_monitoring: vec![],
            encryption_type: "NONE".to_string(),
            key_id: None,
            stream_mode: "PROVISIONED".to_string(),
            warm_throughput_mibps: 0,
            warm_throughput_records: 0,
            pending_update: None,
        };
        state.streams.insert(name.to_string(), stream);
    }

    #[test]
    fn subscribe_to_shard_emits_eventstream_marker_with_records() {
        let state = store_with_sqlite();
        seed_stream(&state, "s");
        let c = ctx();
        // Register a consumer, then write records.
        let stream_arn = "arn:aws:kinesis:us-east-1:000000000000:stream/s";
        let reg = register_stream_consumer(
            &state,
            &json!({ "StreamARN": stream_arn, "ConsumerName": "efo" }),
            &c,
        )
        .unwrap();
        let consumer_arn = reg["Consumer"]["ConsumerARN"].as_str().unwrap().to_string();

        let sqlite = state.sqlite().unwrap().clone();
        for seq in 1..=3 {
            sqlite
                .put_record(
                    "000000000000",
                    "us-east-1",
                    "s",
                    "shardId-000000000000",
                    seq,
                    "pk",
                    "ZGF0YQ==",
                    seq * 1000,
                )
                .unwrap();
        }

        let resp = subscribe_to_shard(
            &state,
            &json!({
                "ConsumerARN": consumer_arn,
                "ShardId": "shardId-000000000000",
                "StartingPosition": { "Type": "TRIM_HORIZON" },
            }),
            &c,
        )
        .unwrap();

        // Protocol-layer marker shape with a single SubscribeToShardEvent.
        let frames = resp["__awsim_eventstream__"].as_array().unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(
            frame["headers"][":event-type"].as_str(),
            Some("SubscribeToShardEvent")
        );
        let records = frame["payload"]["Records"].as_array().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[0]["SequenceNumber"].as_str(),
            Some("00000000000000000001")
        );
        assert_eq!(
            frame["payload"]["ContinuationSequenceNumber"].as_str(),
            Some("00000000000000000003")
        );
        assert!(
            frame["payload"]["ChildShards"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        // The marker encodes to a non-empty event-stream binary buffer.
        let bytes = awsim_core::protocol::eventstream::try_encode(&resp).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn subscribe_to_shard_latest_skips_existing_records() {
        let state = store_with_sqlite();
        seed_stream(&state, "s");
        let c = ctx();
        let stream_arn = "arn:aws:kinesis:us-east-1:000000000000:stream/s";
        let reg = register_stream_consumer(
            &state,
            &json!({ "StreamARN": stream_arn, "ConsumerName": "efo" }),
            &c,
        )
        .unwrap();
        let consumer_arn = reg["Consumer"]["ConsumerARN"].as_str().unwrap().to_string();

        let sqlite = state.sqlite().unwrap().clone();
        for seq in 1..=2 {
            sqlite
                .put_record(
                    "000000000000",
                    "us-east-1",
                    "s",
                    "shardId-000000000000",
                    seq,
                    "pk",
                    "ZA==",
                    seq * 10,
                )
                .unwrap();
        }

        let resp = subscribe_to_shard(
            &state,
            &json!({
                "ConsumerARN": consumer_arn,
                "ShardId": "shardId-000000000000",
                "StartingPosition": { "Type": "LATEST" },
            }),
            &c,
        )
        .unwrap();
        let records = resp["__awsim_eventstream__"][0]["payload"]["Records"]
            .as_array()
            .unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn sweep_removes_idle_consumer_keeps_fresh() {
        let state = KinesisState::default();
        let now = 10_000u64;
        state.consumers.insert(
            "arn:idle".to_string(),
            StreamConsumer {
                consumer_arn: "arn:idle".to_string(),
                consumer_name: "idle".to_string(),
                consumer_status: "ACTIVE".to_string(),
                stream_arn: "arn:stream".to_string(),
                consumer_creation_timestamp: 0,
                last_active_secs: now - CONSUMER_IDLE_SECS - 1,
            },
        );
        state.consumers.insert(
            "arn:fresh".to_string(),
            StreamConsumer {
                consumer_arn: "arn:fresh".to_string(),
                consumer_name: "fresh".to_string(),
                consumer_status: "ACTIVE".to_string(),
                stream_arn: "arn:stream".to_string(),
                consumer_creation_timestamp: 0,
                last_active_secs: now - 1,
            },
        );

        let removed = sweep_idle_consumers(&state, now);
        assert_eq!(removed, 1);
        assert!(!state.consumers.contains_key("arn:idle"));
        assert!(state.consumers.contains_key("arn:fresh"));

        // Idempotent: a second sweep with nothing idle removes none.
        assert_eq!(sweep_idle_consumers(&state, now), 0);
    }
}
