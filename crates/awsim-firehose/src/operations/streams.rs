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

    let source_config = validate_source_configuration(&stream_type, input)?;

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
        source_config,
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
    let source = s
        .source_config
        .as_ref()
        .map(|cfg| match s.stream_type.as_str() {
            "KinesisStreamAsSource" => json!({ "KinesisStreamSourceDescription": cfg }),
            "MSKAsSource" => json!({ "MSKSourceDescription": cfg }),
            "DatabaseAsSource" => json!({ "DatabaseSourceDescription": cfg }),
            _ => json!({}),
        });
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
            "Source": source,
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
        && !matches!(
            t,
            "DirectPut" | "KinesisStreamAsSource" | "MSKAsSource" | "DatabaseAsSource"
        )
    {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "DeliveryStreamType `{t}` must be DirectPut, KinesisStreamAsSource, MSKAsSource, or DatabaseAsSource."
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
    let expected_version = input
        .get("CurrentDeliveryStreamVersionId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidArgumentException",
                "CurrentDeliveryStreamVersionId is required",
            )
        })?;
    let destination_id = input
        .get("DestinationId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request("InvalidArgumentException", "DestinationId is required")
        })?;
    let mut s = state.streams.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ResourceNotFoundException",
            format!("Stream {name} not found"),
        )
    })?;
    // AWS rejects edits that race with another caller's update; the
    // caller must echo the latest VersionId observed via Describe.
    if s.version_id != expected_version {
        return Err(AwsError::bad_request(
            "ConcurrentModificationException",
            format!(
                "CurrentDeliveryStreamVersionId `{expected_version}` does not match the current VersionId `{}`.",
                s.version_id,
            ),
        ));
    }
    let known_destination = s
        .destinations
        .iter()
        .any(|d| d.get("DestinationId").and_then(Value::as_str) == Some(destination_id));
    if !known_destination {
        return Err(AwsError::bad_request(
            "ResourceNotFoundException",
            format!("DestinationId `{destination_id}` does not exist on stream {name}."),
        ));
    }
    s.destinations = collect_destinations(input);
    s.last_update_timestamp = now_secs();
    s.version_id = format!("{}", s.version_id.parse::<u64>().unwrap_or(1) + 1);
    Ok(json!({}))
}

/// Validates and captures the source configuration that pairs with
/// `DeliveryStreamType`. Returns the raw config JSON (used verbatim
/// when echoing `Source.<Kind>SourceDescription` on Describe).
/// `DirectPut` rejects any source block; the source-backed types each
/// require the matching `*SourceConfiguration` field.
fn validate_source_configuration(
    stream_type: &str,
    input: &Value,
) -> Result<Option<Value>, AwsError> {
    let kinesis = input.get("KinesisStreamSourceConfiguration");
    let msk = input.get("MSKSourceConfiguration");
    let database = input.get("DatabaseSourceConfiguration");
    let present: Vec<&str> = [
        ("KinesisStreamSourceConfiguration", kinesis.is_some()),
        ("MSKSourceConfiguration", msk.is_some()),
        ("DatabaseSourceConfiguration", database.is_some()),
    ]
    .iter()
    .filter_map(|(n, p)| if *p { Some(*n) } else { None })
    .collect();
    if present.len() > 1 {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "Only one of KinesisStreamSourceConfiguration / MSKSourceConfiguration / DatabaseSourceConfiguration may be set (got {present:?})."
            ),
        ));
    }
    let want = match stream_type {
        "DirectPut" => None,
        "KinesisStreamAsSource" => Some("KinesisStreamSourceConfiguration"),
        "MSKAsSource" => Some("MSKSourceConfiguration"),
        "DatabaseAsSource" => Some("DatabaseSourceConfiguration"),
        _ => return Ok(None),
    };
    match (want, present.first().copied()) {
        (None, Some(n)) => Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("DeliveryStreamType=DirectPut does not accept {n}."),
        )),
        (Some(expected), None) => Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("DeliveryStreamType={stream_type} requires {expected}."),
        )),
        (Some(expected), Some(actual)) if expected != actual => Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("DeliveryStreamType={stream_type} requires {expected}, got {actual}."),
        )),
        (Some(_), Some(_)) => {
            let cfg = kinesis.or(msk).or(database).cloned().unwrap_or_default();
            match want {
                Some("KinesisStreamSourceConfiguration") => validate_kinesis_source(&cfg)?,
                Some("MSKSourceConfiguration") => validate_msk_source(&cfg)?,
                _ => {}
            }
            Ok(Some(cfg))
        }
        (None, None) => Ok(None),
    }
}

/// Validates that `KinesisStreamARN` is a Kinesis stream ARN and
/// `RoleARN` is an IAM role ARN. Both are required per the AWS
/// Firehose API reference.
fn validate_kinesis_source(cfg: &Value) -> Result<(), AwsError> {
    let stream_arn = cfg
        .get("KinesisStreamARN")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidArgumentException",
                "KinesisStreamSourceConfiguration.KinesisStreamARN is required.",
            )
        })?;
    if !is_service_arn(stream_arn, "kinesis", "stream/") {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "KinesisStreamSourceConfiguration.KinesisStreamARN `{stream_arn}` must be a Kinesis stream ARN."
            ),
        ));
    }
    let role_arn = cfg.get("RoleARN").and_then(Value::as_str).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidArgumentException",
            "KinesisStreamSourceConfiguration.RoleARN is required.",
        )
    })?;
    if !is_iam_role_arn(role_arn) {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "KinesisStreamSourceConfiguration.RoleARN `{role_arn}` must be an IAM role ARN."
            ),
        ));
    }
    Ok(())
}

/// Validates the inner fields of `MSKSourceConfiguration`: a Kafka
/// cluster ARN, a non-empty TopicName, and an
/// AuthenticationConfiguration whose Connectivity enum is well-formed.
fn validate_msk_source(cfg: &Value) -> Result<(), AwsError> {
    let cluster_arn = cfg
        .get("MSKClusterARN")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidArgumentException",
                "MSKSourceConfiguration.MSKClusterARN is required.",
            )
        })?;
    if !is_service_arn(cluster_arn, "kafka", "cluster/") {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "MSKSourceConfiguration.MSKClusterARN `{cluster_arn}` must be a Kafka (MSK) cluster ARN."
            ),
        ));
    }
    let topic = cfg
        .get("TopicName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidArgumentException",
                "MSKSourceConfiguration.TopicName is required.",
            )
        })?;
    if topic.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            "MSKSourceConfiguration.TopicName must not be empty.",
        ));
    }
    let auth = cfg.get("AuthenticationConfiguration").ok_or_else(|| {
        AwsError::bad_request(
            "InvalidArgumentException",
            "MSKSourceConfiguration.AuthenticationConfiguration is required.",
        )
    })?;
    let connectivity = auth
        .get("Connectivity")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidArgumentException",
                "AuthenticationConfiguration.Connectivity is required.",
            )
        })?;
    if !matches!(connectivity, "PUBLIC" | "PRIVATE") {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!(
                "AuthenticationConfiguration.Connectivity `{connectivity}` must be PUBLIC or PRIVATE."
            ),
        ));
    }
    let role_arn = auth.get("RoleARN").and_then(Value::as_str).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidArgumentException",
            "AuthenticationConfiguration.RoleARN is required.",
        )
    })?;
    if !is_iam_role_arn(role_arn) {
        return Err(AwsError::bad_request(
            "InvalidArgumentException",
            format!("AuthenticationConfiguration.RoleARN `{role_arn}` must be an IAM role ARN."),
        ));
    }
    Ok(())
}

fn is_service_arn(arn: &str, service: &str, resource_prefix: &str) -> bool {
    let mut it = arn.splitn(6, ':');
    matches!(
        (it.next(), it.next(), it.next(), it.next(), it.next(), it.next()),
        (Some("arn"), Some(p), Some(svc), Some(_region), Some(account), Some(resource))
            if !p.is_empty()
                && svc == service
                && account.len() == 12
                && account.chars().all(|c| c.is_ascii_digit())
                && resource.starts_with(resource_prefix)
                && resource.len() > resource_prefix.len()
    )
}

fn is_iam_role_arn(arn: &str) -> bool {
    let mut it = arn.splitn(6, ':');
    matches!(
        (it.next(), it.next(), it.next(), it.next(), it.next(), it.next()),
        (Some("arn"), Some(p), Some("iam"), Some(""), Some(account), Some(resource))
            if !p.is_empty()
                && account.len() == 12
                && account.chars().all(|c| c.is_ascii_digit())
                && resource.starts_with("role/")
                && resource.len() > "role/".len()
    )
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
            source_config: None,
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

    #[test]
    fn create_rejects_source_config_on_direct_put() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-bad",
                "DeliveryStreamType": "DirectPut",
                "KinesisStreamSourceConfiguration": { "KinesisStreamARN": "arn:aws:kinesis:us-east-1:111:stream/x", "RoleARN": "arn:aws:iam::111:role/r" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_kinesis_source_without_config() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-missing",
                "DeliveryStreamType": "KinesisStreamAsSource",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_mismatched_source_config() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-mismatch",
                "DeliveryStreamType": "MSKAsSource",
                "KinesisStreamSourceConfiguration": { "KinesisStreamARN": "arn:aws:kinesis:us-east-1:111:stream/x", "RoleARN": "arn:aws:iam::111:role/r" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_kinesis_source_with_bad_stream_arn() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-arn",
                "DeliveryStreamType": "KinesisStreamAsSource",
                "KinesisStreamSourceConfiguration": {
                    "KinesisStreamARN": "not-an-arn",
                    "RoleARN": "arn:aws:iam::111111111111:role/firehose-src",
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_kinesis_source_with_bad_role_arn() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-role",
                "DeliveryStreamType": "KinesisStreamAsSource",
                "KinesisStreamSourceConfiguration": {
                    "KinesisStreamARN": "arn:aws:kinesis:us-east-1:111111111111:stream/in",
                    "RoleARN": "arn:aws:iam::111111111111:user/joe",
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_kinesis_source_missing_required_fields() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-empty",
                "DeliveryStreamType": "KinesisStreamAsSource",
                "KinesisStreamSourceConfiguration": {},
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn list_delivery_streams_accepts_database_as_source_filter() {
        let state = FirehoseState::default();
        let resp = list_delivery_streams(
            &state,
            &json!({ "DeliveryStreamType": "DatabaseAsSource" }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["DeliveryStreamNames"].as_array().unwrap().is_empty());
    }

    #[test]
    fn create_accepts_msk_source_with_valid_arns() {
        let state = FirehoseState::default();
        create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-msk-ok",
                "DeliveryStreamType": "MSKAsSource",
                "MSKSourceConfiguration": {
                    "MSKClusterARN": "arn:aws:kafka:us-east-1:111111111111:cluster/topic/abc",
                    "TopicName": "events",
                    "AuthenticationConfiguration": {
                        "Connectivity": "PRIVATE",
                        "RoleARN": "arn:aws:iam::111111111111:role/firehose-msk",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_rejects_msk_source_with_bad_connectivity() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-msk-bad",
                "DeliveryStreamType": "MSKAsSource",
                "MSKSourceConfiguration": {
                    "MSKClusterARN": "arn:aws:kafka:us-east-1:111111111111:cluster/topic/abc",
                    "TopicName": "events",
                    "AuthenticationConfiguration": {
                        "Connectivity": "INTERNET",
                        "RoleARN": "arn:aws:iam::111111111111:role/firehose-msk",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_rejects_msk_source_with_kinesis_arn() {
        let state = FirehoseState::default();
        let err = create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-msk-arn",
                "DeliveryStreamType": "MSKAsSource",
                "MSKSourceConfiguration": {
                    "MSKClusterARN": "arn:aws:kinesis:us-east-1:111111111111:stream/x",
                    "TopicName": "events",
                    "AuthenticationConfiguration": {
                        "Connectivity": "PUBLIC",
                        "RoleARN": "arn:aws:iam::111111111111:role/firehose-msk",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn update_destination_rejects_stale_version_id() {
        let state = FirehoseState::default();
        create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-occ",
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::b" },
            }),
            &ctx(),
        )
        .unwrap();
        let described =
            describe_delivery_stream(&state, &json!({ "DeliveryStreamName": "ds-occ" }), &ctx())
                .unwrap();
        let destination_id =
            described["DeliveryStreamDescription"]["Destinations"][0]["DestinationId"]
                .as_str()
                .unwrap()
                .to_string();
        let err = update_destination(
            &state,
            &json!({
                "DeliveryStreamName": "ds-occ",
                "CurrentDeliveryStreamVersionId": "9",
                "DestinationId": destination_id,
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::b" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ConcurrentModificationException");
    }

    #[test]
    fn update_destination_rejects_unknown_destination_id() {
        let state = FirehoseState::default();
        create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-bad-dest",
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::b" },
            }),
            &ctx(),
        )
        .unwrap();
        let err = update_destination(
            &state,
            &json!({
                "DeliveryStreamName": "ds-bad-dest",
                "CurrentDeliveryStreamVersionId": "1",
                "DestinationId": "ghost",
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::b" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn extended_s3_fields_round_trip_via_create_and_update() {
        let state = FirehoseState::default();
        let cfg = json!({
            "BucketARN": "arn:aws:s3:::data",
            "RoleARN": "arn:aws:iam::111111111111:role/firehose",
            "BufferingHints": { "SizeInMBs": 64, "IntervalInSeconds": 300 },
            "CompressionFormat": "GZIP",
            "EncryptionConfiguration": {
                "KMSEncryptionConfig": { "AWSKMSKeyARN": "arn:aws:kms:us-east-1:111111111111:key/abc" }
            },
            "CloudWatchLoggingOptions": {
                "Enabled": true,
                "LogGroupName": "/aws/firehose/data",
                "LogStreamName": "S3Delivery",
            },
            "DataFormatConversionConfiguration": {
                "Enabled": true,
                "SchemaConfiguration": { "RoleARN": "arn:aws:iam::111111111111:role/glue" },
            },
            "DynamicPartitioningConfiguration": {
                "Enabled": true,
                "RetryOptions": { "DurationInSeconds": 300 },
            },
            "FileExtension": ".log.gz",
            "CustomTimeZone": "UTC",
        });
        create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ext-roundtrip",
                "ExtendedS3DestinationConfiguration": cfg,
            }),
            &ctx(),
        )
        .unwrap();
        let described = describe_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "ext-roundtrip" }),
            &ctx(),
        )
        .unwrap();
        let dest = &described["DeliveryStreamDescription"]["Destinations"][0]["ExtendedS3DestinationDescription"];
        assert_eq!(dest["CompressionFormat"], "GZIP");
        assert_eq!(dest["FileExtension"], ".log.gz");
        assert_eq!(dest["CustomTimeZone"], "UTC");
        assert_eq!(dest["BufferingHints"]["SizeInMBs"], 64);
        assert_eq!(dest["DynamicPartitioningConfiguration"]["Enabled"], true);
        let destination_id =
            described["DeliveryStreamDescription"]["Destinations"][0]["DestinationId"]
                .as_str()
                .unwrap()
                .to_string();
        let updated_cfg = json!({
            "BucketARN": "arn:aws:s3:::data",
            "BufferingHints": { "SizeInMBs": 16, "IntervalInSeconds": 60 },
            "CompressionFormat": "Snappy",
            "FileExtension": ".snappy",
            "CustomTimeZone": "America/Los_Angeles",
        });
        update_destination(
            &state,
            &json!({
                "DeliveryStreamName": "ext-roundtrip",
                "CurrentDeliveryStreamVersionId": "1",
                "DestinationId": destination_id,
                "ExtendedS3DestinationConfiguration": updated_cfg,
            }),
            &ctx(),
        )
        .unwrap();
        let described = describe_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "ext-roundtrip" }),
            &ctx(),
        )
        .unwrap();
        let dest = &described["DeliveryStreamDescription"]["Destinations"][0]["ExtendedS3DestinationDescription"];
        assert_eq!(dest["CompressionFormat"], "Snappy");
        assert_eq!(dest["FileExtension"], ".snappy");
        assert_eq!(dest["CustomTimeZone"], "America/Los_Angeles");
        assert_eq!(dest["BufferingHints"]["SizeInMBs"], 16);
    }

    #[test]
    fn create_persists_source_config_and_describe_echoes_it() {
        let state = FirehoseState::default();
        create_delivery_stream(
            &state,
            &json!({
                "DeliveryStreamName": "ds-kinesis",
                "DeliveryStreamType": "KinesisStreamAsSource",
                "KinesisStreamSourceConfiguration": {
                    "KinesisStreamARN": "arn:aws:kinesis:us-east-1:111111111111:stream/in",
                    "RoleARN": "arn:aws:iam::111111111111:role/firehose-src",
                },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_delivery_stream(
            &state,
            &json!({ "DeliveryStreamName": "ds-kinesis" }),
            &ctx(),
        )
        .unwrap();
        let desc = &resp["DeliveryStreamDescription"]["Source"]["KinesisStreamSourceDescription"];
        assert_eq!(
            desc["KinesisStreamARN"],
            "arn:aws:kinesis:us-east-1:111111111111:stream/in"
        );
    }
}
