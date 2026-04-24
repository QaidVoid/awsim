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
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Delivery stream {name} already exists"),
        ));
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
        stream_type: input["DeliveryStreamType"]
            .as_str()
            .unwrap_or("DirectPut")
            .to_string(),
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
        AwsError::not_found(
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
        AwsError::not_found(
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
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = state.streams.iter().map(|e| e.key().clone()).collect();
    Ok(json!({
        "DeliveryStreamNames": names,
        "HasMoreDeliveryStreams": false,
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    s.destinations = collect_destinations(input);
    s.last_update_timestamp = now_secs();
    s.version_id = format!("{}", s.version_id.parse::<u64>().unwrap_or(1) + 1);
    Ok(json!({}))
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
