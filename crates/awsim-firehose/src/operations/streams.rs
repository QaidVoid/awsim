use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DeliveryStream, FirehoseState, now_secs};

pub fn create_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    if state.streams.contains_key(name) {
        return Err(AwsError::bad_request(
            "ResourceInUseException",
            format!("Delivery stream {name} already exists"),
        ));
    }

    let stream_type = input["DeliveryStreamType"]
        .as_str()
        .unwrap_or("DirectPut")
        .to_string();
    if !matches!(
        stream_type.as_str(),
        "DirectPut" | "KinesisStreamAsSource" | "MSKAsSource" | "DatabaseAsSource"
    ) {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "DeliveryStreamType `{stream_type}` must be DirectPut, KinesisStreamAsSource, MSKAsSource, or DatabaseAsSource."
            ),
        ));
    }

    if let Some(ext) = input.get("ExtendedS3DestinationConfiguration") {
        validate_extended_s3(ext)?;
    }

    let arn = format!(
        "arn:aws:firehose:{}:{}:deliverystream/{}",
        ctx.region, ctx.account_id, name
    );
    let destinations = collect_destinations(input);
    let stream = DeliveryStream {
        name: name.to_string(),
        arn: arn.clone(),
        status: "ACTIVE".to_string(),
        stream_type,
        version_id: "1".to_string(),
        create_timestamp: now_secs(),
        last_update_timestamp: now_secs(),
        destinations,
        has_more_destinations: false,
        tags: input["Tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        let k = t["Key"].as_str()?.to_string();
                        let v = t["Value"].as_str().unwrap_or("").to_string();
                        Some((k, v))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        encryption_enabled: false,
        encryption_key_type: None,
        encryption_key_arn: None,
    };
    state.streams.insert(name.to_string(), stream);
    Ok(json!({ "DeliveryStreamARN": arn }))
}

pub fn delete_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    state.streams.remove(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    Ok(json!({}))
}

pub fn describe_delivery_stream(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let s = state.streams.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    Ok(json!({
        "DeliveryStreamDescription": {
            "DeliveryStreamName": s.name,
            "DeliveryStreamARN": s.arn,
            "DeliveryStreamStatus": s.status,
            "DeliveryStreamType": s.stream_type,
            "VersionId": s.version_id,
            "CreateTimestamp": s.create_timestamp,
            "LastUpdateTimestamp": s.last_update_timestamp,
            "Destinations": s.destinations,
            "HasMoreDestinations": s.has_more_destinations,
            "DeliveryStreamEncryptionConfiguration": {
                "Status": if s.encryption_enabled { "ENABLED" } else { "DISABLED" },
                "KeyType": s.encryption_key_type,
                "KeyARN": s.encryption_key_arn,
            }
        }
    }))
}

pub fn list_delivery_streams(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // AWS bounds Limit at [1, 10_000] with a default of 10.
    let limit = match input.get("Limit").and_then(Value::as_i64) {
        Some(n) if !(1..=10_000).contains(&n) => {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                "Limit must be between 1 and 10000.",
            ));
        }
        Some(n) => n as usize,
        None => 10,
    };
    let type_filter = input.get("DeliveryStreamType").and_then(Value::as_str);
    if let Some(t) = type_filter
        && !matches!(t, "DirectPut" | "KinesisStreamAsSource" | "MSKAsSource")
    {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "DeliveryStreamType `{t}` must be DirectPut, KinesisStreamAsSource, or MSKAsSource."
            ),
        ));
    }
    let start_after = input
        .get("ExclusiveStartDeliveryStreamName")
        .and_then(Value::as_str);

    let mut names: Vec<String> = state
        .streams
        .iter()
        .filter(|e| type_filter.is_none_or(|t| e.value().stream_type == t))
        .map(|e| e.key().clone())
        .collect();
    names.sort();
    if let Some(start) = start_after {
        let idx = names
            .iter()
            .position(|n| n == start)
            .map(|i| i + 1)
            .unwrap_or(0);
        names.drain(..idx);
    }
    let has_more = names.len() > limit;
    names.truncate(limit);
    Ok(json!({
        "DeliveryStreamNames": names,
        "HasMoreDeliveryStreams": has_more,
    }))
}

pub fn update_destination(
    state: &FirehoseState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["DeliveryStreamName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidArgumentException", "DeliveryStreamName is required")
    })?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    s.destinations = collect_destinations(input);
    s.last_update_timestamp = now_secs();
    s.version_id = format!("{}", s.version_id.parse::<u64>().unwrap_or(1) + 1);
    Ok(json!({}))
}

/// Validate ExtendedS3DestinationConfiguration: BufferingHints size
/// in `[1, 128]` MiB and interval in `[60, 900]` s, and
/// CompressionFormat from the documented allowlist.
fn validate_extended_s3(cfg: &Value) -> Result<(), AwsError> {
    if let Some(b) = cfg.get("BufferingHints") {
        if let Some(size) = b.get("SizeInMBs").and_then(Value::as_u64)
            && !(1..=128).contains(&size)
        {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!("BufferingHints.SizeInMBs must be between 1 and 128 (got {size})."),
            ));
        }
        if let Some(secs) = b.get("IntervalInSeconds").and_then(Value::as_u64)
            && !(60..=900).contains(&secs)
        {
            return Err(AwsError::bad_request(
                "InvalidArgumentException",
                format!(
                    "BufferingHints.IntervalInSeconds must be between 60 and 900 (got {secs})."
                ),
            ));
        }
    }
    if let Some(cf) = cfg.get("CompressionFormat").and_then(Value::as_str)
        && !matches!(
            cf,
            "UNCOMPRESSED" | "GZIP" | "Snappy" | "HADOOP_SNAPPY" | "ZIP"
        )
    {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "CompressionFormat `{cf}` must be UNCOMPRESSED, GZIP, Snappy, HADOOP_SNAPPY, or ZIP."
            ),
        ));
    }
    Ok(())
}

fn collect_destinations(input: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(s3) = input.get("S3DestinationConfiguration").cloned() {
        out.push(json!({ "DestinationId": id.clone(), "S3DestinationDescription": s3 }));
    }
    if let Some(ext) = input.get("ExtendedS3DestinationConfiguration").cloned() {
        out.push(json!({ "DestinationId": id.clone(), "ExtendedS3DestinationDescription": ext }));
    }
    if let Some(rs) = input.get("RedshiftDestinationConfiguration").cloned() {
        out.push(json!({ "DestinationId": id.clone(), "RedshiftDestinationDescription": rs }));
    }
    if let Some(es) = input.get("ElasticsearchDestinationConfiguration").cloned() {
        out.push(json!({ "DestinationId": id.clone(), "ElasticsearchDestinationDescription": es }));
    }
    if let Some(http) = input.get("HttpEndpointDestinationConfiguration").cloned() {
        out.push(
            json!({ "DestinationId": id.clone(), "HttpEndpointDestinationDescription": http }),
        );
    }
    if let Some(sf) = input.get("SnowflakeDestinationConfiguration").cloned() {
        out.push(json!({ "DestinationId": id, "SnowflakeDestinationDescription": sf }));
    }
    out
}

#[cfg(test)]
mod list_delivery_streams_tests {
    use super::*;
    use crate::state::DeliveryStream;

    fn ctx() -> RequestContext {
        RequestContext::new("firehose", "us-east-1")
    }

    fn put_stream(state: &FirehoseState, name: &str, stream_type: &str) {
        let s = DeliveryStream {
            name: name.to_string(),
            arn: format!("arn:aws:firehose:us-east-1:000000000000:deliverystream/{name}"),
            status: "ACTIVE".to_string(),
            stream_type: stream_type.to_string(),
            version_id: "1".to_string(),
            create_timestamp: 0,
            last_update_timestamp: 0,
            destinations: vec![],
            has_more_destinations: false,
            tags: Default::default(),
            encryption_enabled: false,
            encryption_key_type: None,
            encryption_key_arn: None,
        };
        state.streams.insert(name.to_string(), s);
    }

    #[test]
    fn paginates_with_limit_and_exclusive_start() {
        let state = FirehoseState::default();
        for n in ["a", "b", "c", "d", "e"] {
            put_stream(&state, n, "DirectPut");
        }
        let resp = list_delivery_streams(&state, &json!({ "Limit": 2 }), &ctx()).unwrap();
        assert_eq!(resp["HasMoreDeliveryStreams"], json!(true));
        let names = resp["DeliveryStreamNames"].as_array().unwrap();
        assert_eq!(names.len(), 2);
        let last = names.last().unwrap().as_str().unwrap();

        let resp = list_delivery_streams(
            &state,
            &json!({ "Limit": 10, "ExclusiveStartDeliveryStreamName": last }),
            &ctx(),
        )
        .unwrap();
        let names2 = resp["DeliveryStreamNames"].as_array().unwrap();
        assert_eq!(names2.len(), 3);
    }

    #[test]
    fn filters_by_stream_type() {
        let state = FirehoseState::default();
        put_stream(&state, "a", "DirectPut");
        put_stream(&state, "b", "KinesisStreamAsSource");
        let resp = list_delivery_streams(
            &state,
            &json!({ "DeliveryStreamType": "KinesisStreamAsSource" }),
            &ctx(),
        )
        .unwrap();
        let names = resp["DeliveryStreamNames"].as_array().unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "b");
    }

    #[test]
    fn rejects_unknown_stream_type() {
        let state = FirehoseState::default();
        let err = list_delivery_streams(&state, &json!({ "DeliveryStreamType": "Bogus" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn rejects_limit_out_of_range() {
        let state = FirehoseState::default();
        let err = list_delivery_streams(&state, &json!({ "Limit": 0 }), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }
}
