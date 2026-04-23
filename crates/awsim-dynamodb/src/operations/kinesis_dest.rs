use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DynamoState, KinesisStreamingDestination};

use super::require_str;

pub fn enable_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let stream_arn = require_str(input, "StreamArn")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let precision = input
        .get("EnableKinesisStreamingConfiguration")
        .and_then(|v| v.get("ApproximateCreationDateTimePrecision"))
        .and_then(|v| v.as_str())
        .unwrap_or("MICROSECOND")
        .to_string();

    let entry = KinesisStreamingDestination {
        stream_arn: stream_arn.to_string(),
        destination_status: "ACTIVE".to_string(),
        approximate_creation_date_time_precision: precision.clone(),
    };

    state
        .kinesis_destinations
        .entry(table_name.to_string())
        .or_default()
        .push(entry);

    Ok(json!({
        "TableName": table_name,
        "StreamArn": stream_arn,
        "DestinationStatus": "ENABLING",
        "EnableKinesisStreamingConfiguration": {
            "ApproximateCreationDateTimePrecision": precision
        }
    }))
}

pub fn disable_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let stream_arn = require_str(input, "StreamArn")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    if let Some(mut list) = state.kinesis_destinations.get_mut(table_name) {
        list.retain(|d| d.stream_arn != stream_arn);
    }

    Ok(json!({
        "TableName": table_name,
        "StreamArn": stream_arn,
        "DestinationStatus": "DISABLING"
    }))
}

pub fn describe_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let destinations: Vec<Value> = state
        .kinesis_destinations
        .get(table_name)
        .map(|list| {
            list.iter()
                .map(|d| {
                    json!({
                        "StreamArn": d.stream_arn,
                        "DestinationStatus": d.destination_status,
                        "ApproximateCreationDateTimePrecision": d.approximate_creation_date_time_precision
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "TableName": table_name,
        "KinesisDataStreamDestinations": destinations
    }))
}
