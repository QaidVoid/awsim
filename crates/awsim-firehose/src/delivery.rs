//! S3 delivery for Firehose streams. A `PutRecord` / `PutRecordBatch`
//! against a stream with an (Extended)S3 destination delivers the batch
//! as one object, keyed by the AWS layout
//! `<Prefix><yyyy>/<MM>/<dd>/<HH>/<DeliveryStreamName>-<DestinationId>-<UUID>`.
//! When a Lambda processor is configured the records are transformed
//! first; transformed records go to `Prefix`, processing failures to
//! `ErrorOutputPrefix`. Object bytes are newline-delimited and written
//! uncompressed (CompressionFormat is metadata only).

use std::sync::Arc;

use awsim_core::{LambdaInvoker, S3ObjectWriter};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::processors::run_processors;
use crate::state::{FirehoseState, now_secs};

/// Build the AWS Firehose S3 object key for a delivered batch.
pub fn s3_object_key(
    prefix: &str,
    ts: u64,
    stream: &str,
    dest_id: &str,
    file_ext: Option<&str>,
) -> String {
    let dt = chrono::DateTime::from_timestamp(ts as i64, 0).unwrap_or_default();
    let path = dt.format("%Y/%m/%d/%H");
    let mut key = format!("{prefix}{path}/{stream}-{dest_id}-{}", uuid::Uuid::new_v4());
    if let Some(ext) = file_ext {
        key.push_str(ext);
    }
    key
}

fn bucket_from_arn(arn: &str) -> &str {
    arn.strip_prefix("arn:aws:s3:::").unwrap_or(arn)
}

/// Newline-delimit the decoded record bytes; undecodable records are
/// skipped.
fn concat_records(records: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        if let Ok(mut b) = BASE64.decode(r) {
            out.append(&mut b);
            out.push(b'\n');
        }
    }
    out
}

/// Extract the Lambda processor ARN from an (Extended)S3 destination
/// description, if a Lambda processor is configured.
fn lambda_processor_arn(desc: &Value) -> Option<String> {
    desc.get("ProcessingConfiguration")?
        .get("Processors")?
        .as_array()?
        .iter()
        .find(|p| p.get("Type").and_then(Value::as_str) == Some("Lambda"))?
        .get("Parameters")?
        .as_array()?
        .iter()
        .find(|kv| kv.get("ParameterName").and_then(Value::as_str) == Some("LambdaArn"))?
        .get("ParameterValue")
        .and_then(Value::as_str)
        .map(String::from)
}

/// Deliver a batch of base64-encoded records to a stream's (Extended)S3
/// destination. No-op when the stream has no S3 destination, no S3
/// writer is wired, or the records are empty.
pub fn deliver_records(
    state: &FirehoseState,
    s3_writer: Option<&Arc<dyn S3ObjectWriter>>,
    lambda_invoker: Option<&Arc<dyn LambdaInvoker>>,
    stream_name: &str,
    records: &[String],
    account: &str,
    region: &str,
) {
    if records.is_empty() {
        return;
    }
    let Some(writer) = s3_writer else {
        return;
    };

    // Resolve the S3 destination + its config from the stream.
    let (bucket, prefix, error_prefix, file_ext, dest_id, lambda_arn) = {
        let Some(stream) = state.streams.get(stream_name) else {
            return;
        };
        let Some(dest) = stream.destinations.iter().find(|d| {
            d.get("ExtendedS3DestinationDescription").is_some()
                || d.get("S3DestinationDescription").is_some()
        }) else {
            return;
        };
        let desc = dest
            .get("ExtendedS3DestinationDescription")
            .or_else(|| dest.get("S3DestinationDescription"))
            .cloned()
            .unwrap_or_default();
        let bucket = bucket_from_arn(desc.get("BucketARN").and_then(Value::as_str).unwrap_or(""))
            .to_string();
        if bucket.is_empty() {
            return;
        }
        (
            bucket,
            desc.get("Prefix")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            desc.get("ErrorOutputPrefix")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            desc.get("FileExtension")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
            dest.get("DestinationId")
                .and_then(Value::as_str)
                .unwrap_or("destination-1")
                .to_string(),
            lambda_processor_arn(&desc),
        )
    };

    // Run the Lambda transform if configured and an invoker is wired.
    let (transformed, failed, dropped) = match (lambda_arn.as_deref(), lambda_invoker) {
        (Some(arn), Some(inv)) => {
            let o = run_processors(inv.as_ref(), arn, records, account, region);
            (o.transformed, o.failed, o.dropped)
        }
        _ => (records.to_vec(), Vec::new(), 0),
    };

    let ts = now_secs();
    let mut stats = state
        .delivery_stats
        .entry(stream_name.to_string())
        .or_default();

    if !transformed.is_empty() {
        let key = s3_object_key(&prefix, ts, stream_name, &dest_id, file_ext.as_deref());
        let blob = concat_records(&transformed);
        if writer
            .put_object(&bucket, &key, &BASE64.encode(&blob), account, region)
            .is_ok()
        {
            stats.last_s3_keys.push(key);
            stats.succeeded_records += transformed.len() as u64;
        }
    }
    if !failed.is_empty() {
        // Failed records land under ErrorOutputPrefix (falling back to
        // Prefix when it is unset).
        let eprefix = if error_prefix.is_empty() {
            &prefix
        } else {
            &error_prefix
        };
        let key = s3_object_key(eprefix, ts, stream_name, &dest_id, file_ext.as_deref());
        let blob = concat_records(&failed);
        if writer
            .put_object(&bucket, &key, &BASE64.encode(&blob), account, region)
            .is_ok()
        {
            stats.last_s3_keys.push(key);
        }
        stats.processing_failed += failed.len() as u64;
    }
    stats.processing_dropped += dropped;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_layout_matches_aws_format() {
        let key = s3_object_key("raw/", 1_700_000_000, "my-stream", "dest-1", Some(".json"));
        // raw/2023/11/14/22/my-stream-dest-1-<uuid>.json
        assert!(key.starts_with("raw/2023/11/"), "key was {key}");
        assert!(key.contains("/my-stream-dest-1-"), "key was {key}");
        assert!(key.ends_with(".json"), "key was {key}");
    }

    #[test]
    fn empty_prefix_starts_with_date() {
        let key = s3_object_key("", 1_700_000_000, "s", "d", None);
        assert!(key.starts_with("2023/"), "key was {key}");
    }
}
