use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::state::FirehoseState;

/// AWS rejects PutRecord (and per-record entries within PutRecordBatch)
/// whose data exceeds 1 MiB before base64 encoding. The batch overall
/// caps at 500 records and 4 MiB total decoded size.
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_BATCH_RECORDS: usize = 500;
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;

fn decoded_data_len(record: &Value) -> Option<usize> {
    let s = record.get("Data")?.as_str()?;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .ok()
        .map(|b| b.len())
}

pub fn put_record(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let stream = state.streams.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    let record = input
        .get("Record")
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "Record is required"))?;
    if let Some(len) = decoded_data_len(record)
        && len > MAX_RECORD_BYTES
    {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("Record.Data is {len} bytes; maximum is {MAX_RECORD_BYTES} (1 MiB)."),
        ));
    }
    let encrypted = stream.encryption_enabled;
    let record_id = uuid::Uuid::new_v4().to_string();
    Ok(json!({
        "RecordId": record_id,
        "Encrypted": encrypted,
    }))
}

pub fn put_record_batch(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let stream = state.streams.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;

    let records = input["Records"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidArgumentException", "Records is required"))?;

    if records.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "Records must contain at least one entry.",
        ));
    }
    if records.len() > MAX_BATCH_RECORDS {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "Records contains {} entries; maximum is {MAX_BATCH_RECORDS} per PutRecordBatch.",
                records.len()
            ),
        ));
    }

    let mut total_bytes = 0usize;
    let mut entries: Vec<Value> = Vec::with_capacity(records.len());
    let mut failed = 0u64;
    for r in records {
        let len = decoded_data_len(r);
        if let Some(n) = len {
            total_bytes = total_bytes.saturating_add(n);
        }
        if matches!(len, Some(n) if n > MAX_RECORD_BYTES) || len.is_none() {
            failed += 1;
            entries.push(json!({
                "ErrorCode": "InvalidArgumentException",
                "ErrorMessage": format!(
                    "Record.Data must be base64-encoded and at most {MAX_RECORD_BYTES} bytes."
                ),
            }));
        } else {
            entries.push(json!({
                "RecordId": uuid::Uuid::new_v4().to_string(),
            }));
        }
    }
    if total_bytes > MAX_BATCH_BYTES {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "Records total decoded size is {total_bytes} bytes; maximum is {MAX_BATCH_BYTES} (4 MiB)."
            ),
        ));
    }

    Ok(json!({
        "FailedPutCount": failed,
        "Encrypted": stream.encryption_enabled,
        "RequestResponses": entries,
    }))
}

#[cfg(test)]
mod put_record_tests {
    use super::*;
    use crate::state::DeliveryStream;

    fn ctx() -> RequestContext {
        RequestContext::new("firehose", "us-east-1")
    }

    fn seed_stream(state: &FirehoseState, name: &str, encrypted: bool) {
        let s = DeliveryStream {
            name: name.to_string(),
            arn: format!("arn:aws:firehose:us-east-1:000000000000:deliverystream/{name}"),
            status: "ACTIVE".to_string(),
            stream_type: "DirectPut".to_string(),
            version_id: "1".to_string(),
            create_timestamp: 0,
            last_update_timestamp: 0,
            destinations: vec![],
            has_more_destinations: false,
            tags: Default::default(),
            encryption_enabled: encrypted,
            encryption_key_type: if encrypted {
                Some("AWS_OWNED_CMK".to_string())
            } else {
                None
            },
            encryption_key_arn: None,
        };
        state.streams.insert(name.to_string(), s);
    }

    #[test]
    fn put_record_reflects_stream_encryption_state() {
        let state = FirehoseState::default();
        seed_stream(&state, "s", true);
        let data = base64::engine::general_purpose::STANDARD.encode(b"hello");
        let resp = put_record(
            &state,
            &json!({ "DeliveryStreamName": "s", "Record": { "Data": data } }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Encrypted"], true);
    }

    #[test]
    fn put_record_rejects_oversize_record() {
        let state = FirehoseState::default();
        seed_stream(&state, "s", false);
        let big = base64::engine::general_purpose::STANDARD.encode(vec![0u8; MAX_RECORD_BYTES + 1]);
        let err = put_record(
            &state,
            &json!({ "DeliveryStreamName": "s", "Record": { "Data": big } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn put_record_batch_rejects_over_500_records() {
        let state = FirehoseState::default();
        seed_stream(&state, "s", false);
        let tiny = base64::engine::general_purpose::STANDARD.encode(b"x");
        let records: Vec<Value> = (0..501).map(|_| json!({ "Data": tiny })).collect();
        let err = put_record_batch(
            &state,
            &json!({ "DeliveryStreamName": "s", "Records": records }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn put_record_batch_reports_per_record_failure() {
        let state = FirehoseState::default();
        seed_stream(&state, "s", false);
        let ok = base64::engine::general_purpose::STANDARD.encode(b"hi");
        let bad = "not-base64-!@#$%";
        let resp = put_record_batch(
            &state,
            &json!({
                "DeliveryStreamName": "s",
                "Records": [{ "Data": ok }, { "Data": bad }]
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["FailedPutCount"], 1);
        let entries = resp["RequestResponses"].as_array().unwrap();
        assert!(entries[0].get("RecordId").is_some());
        assert_eq!(entries[1]["ErrorCode"], "InvalidArgumentException");
    }
}
