use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DynamoState, KinesisStreamingDestination};

use super::require_str;

/// Read the `ApproximateCreationDateTimePrecision` out of a streaming-config
/// block (used by both Enable and Update).
fn precision_from(input: &Value, config_key: &str) -> Option<String> {
    input
        .get(config_key)
        .and_then(|v| v.get("ApproximateCreationDateTimePrecision"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn require_table(state: &DynamoState, table_name: &str) -> Result<(), AwsError> {
    if state.tables.contains_key(table_name) {
        Ok(())
    } else {
        Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Table '{table_name}' not found"),
        ))
    }
}

pub fn enable_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let stream_arn = require_str(input, "StreamArn")?;
    require_table(state, table_name)?;

    let precision = precision_from(input, "EnableKinesisStreamingConfiguration")
        .unwrap_or_else(|| "MICROSECOND".to_string());

    let mut list = state
        .kinesis_destinations
        .entry(table_name.to_string())
        .or_default();
    // AWS rejects a second destination for a stream that is already wired up;
    // a previously DISABLED entry for the same stream can be re-enabled.
    if list
        .iter()
        .any(|d| d.stream_arn == stream_arn && d.destination_status != "DISABLED")
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "Kinesis streaming destination {stream_arn} is already active for table {table_name}"
            ),
        ));
    }
    list.retain(|d| d.stream_arn != stream_arn);
    list.push(KinesisStreamingDestination {
        stream_arn: stream_arn.to_string(),
        destination_status: "ENABLING".to_string(),
        approximate_creation_date_time_precision: precision.clone(),
    });

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
    require_table(state, table_name)?;

    // Mark the destination DISABLING rather than dropping it; DescribeKinesis
    // settles it to DISABLED and AWS keeps the entry visible afterwards.
    let mut found = false;
    if let Some(mut list) = state.kinesis_destinations.get_mut(table_name) {
        for d in list.iter_mut() {
            if d.stream_arn == stream_arn && d.destination_status != "DISABLED" {
                d.destination_status = "DISABLING".to_string();
                found = true;
            }
        }
    }
    if !found {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("No active Kinesis streaming destination {stream_arn} for table {table_name}"),
        ));
    }

    Ok(json!({
        "TableName": table_name,
        "StreamArn": stream_arn,
        "DestinationStatus": "DISABLING"
    }))
}

pub fn update_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let stream_arn = require_str(input, "StreamArn")?;
    require_table(state, table_name)?;

    let new_precision = precision_from(input, "UpdateKinesisStreamingConfiguration");

    let mut updated: Option<String> = None;
    if let Some(mut list) = state.kinesis_destinations.get_mut(table_name) {
        for d in list.iter_mut() {
            if d.stream_arn == stream_arn && d.destination_status != "DISABLED" {
                if let Some(p) = &new_precision {
                    d.approximate_creation_date_time_precision = p.clone();
                }
                d.destination_status = "UPDATING".to_string();
                updated = Some(d.approximate_creation_date_time_precision.clone());
            }
        }
    }

    let Some(precision) = updated else {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("No active Kinesis streaming destination {stream_arn} for table {table_name}"),
        ));
    };

    Ok(json!({
        "TableName": table_name,
        "StreamArn": stream_arn,
        "DestinationStatus": "UPDATING",
        "UpdateKinesisStreamingConfiguration": {
            "ApproximateCreationDateTimePrecision": precision
        }
    }))
}

pub fn describe_kinesis_streaming_destination(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    require_table(state, table_name)?;

    // Converge transient statuses on read: AWS reports ENABLING / UPDATING /
    // DISABLING briefly and then settles to a steady state. We have no async
    // provisioning, so we settle on the next describe.
    if let Some(mut list) = state.kinesis_destinations.get_mut(table_name) {
        for d in list.iter_mut() {
            let settled = match d.destination_status.as_str() {
                "ENABLING" | "UPDATING" => "ACTIVE",
                "DISABLING" => "DISABLED",
                other => other,
            };
            d.destination_status = settled.to_string();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Table};
    use std::collections::VecDeque;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    fn state_with_table() -> DynamoState {
        let state = DynamoState::default();
        let table = Table {
            name: "t".into(),
            arn: "arn:aws:dynamodb:us-east-1:000000000000:table/t".into(),
            key_schema: vec![KeySchemaElement {
                attribute_name: "pk".into(),
                key_type: "HASH".into(),
            }],
            attribute_definitions: vec![],
            billing_mode: "PAY_PER_REQUEST".into(),
            status: "ACTIVE".into(),
            created_at: 0.0,
            gsi: vec![],
            lsi: vec![],
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
            deletion_protection_enabled: false,
            sse: Default::default(),
            read_capacity_units: 0,
            write_capacity_units: 0,
        };
        state.tables.insert("t".into(), table);
        state
    }

    const ARN: &str = "arn:aws:kinesis:us-east-1:000000000000:stream/s";

    fn describe(state: &DynamoState) -> Value {
        describe_kinesis_streaming_destination(state, &json!({ "TableName": "t" }), &ctx()).unwrap()
    }

    #[test]
    fn enable_then_describe_settles_to_active() {
        let state = state_with_table();
        let resp = enable_kinesis_streaming_destination(
            &state,
            &json!({ "TableName": "t", "StreamArn": ARN }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["DestinationStatus"], json!("ENABLING"));

        let d = describe(&state);
        assert_eq!(
            d["KinesisDataStreamDestinations"][0]["DestinationStatus"],
            json!("ACTIVE")
        );
        // Default precision is MICROSECOND.
        assert_eq!(
            d["KinesisDataStreamDestinations"][0]["ApproximateCreationDateTimePrecision"],
            json!("MICROSECOND")
        );
    }

    #[test]
    fn enable_rejects_duplicate_active_destination() {
        let state = state_with_table();
        let req = json!({ "TableName": "t", "StreamArn": ARN });
        enable_kinesis_streaming_destination(&state, &req, &ctx()).unwrap();
        let err = enable_kinesis_streaming_destination(&state, &req, &ctx()).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn update_changes_precision_and_settles_to_active() {
        let state = state_with_table();
        enable_kinesis_streaming_destination(
            &state,
            &json!({ "TableName": "t", "StreamArn": ARN }),
            &ctx(),
        )
        .unwrap();

        let resp = update_kinesis_streaming_destination(
            &state,
            &json!({
                "TableName": "t",
                "StreamArn": ARN,
                "UpdateKinesisStreamingConfiguration": {
                    "ApproximateCreationDateTimePrecision": "MILLISECOND"
                }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["DestinationStatus"], json!("UPDATING"));
        assert_eq!(
            resp["UpdateKinesisStreamingConfiguration"]["ApproximateCreationDateTimePrecision"],
            json!("MILLISECOND")
        );

        let d = describe(&state);
        assert_eq!(
            d["KinesisDataStreamDestinations"][0]["DestinationStatus"],
            json!("ACTIVE")
        );
        assert_eq!(
            d["KinesisDataStreamDestinations"][0]["ApproximateCreationDateTimePrecision"],
            json!("MILLISECOND")
        );
    }

    #[test]
    fn disable_marks_disabling_then_settles_to_disabled_and_remains() {
        let state = state_with_table();
        enable_kinesis_streaming_destination(
            &state,
            &json!({ "TableName": "t", "StreamArn": ARN }),
            &ctx(),
        )
        .unwrap();

        let resp = disable_kinesis_streaming_destination(
            &state,
            &json!({ "TableName": "t", "StreamArn": ARN }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["DestinationStatus"], json!("DISABLING"));

        let d = describe(&state);
        let dests = d["KinesisDataStreamDestinations"].as_array().unwrap();
        assert_eq!(dests.len(), 1, "disabled destination stays listed");
        assert_eq!(dests[0]["DestinationStatus"], json!("DISABLED"));
    }

    #[test]
    fn update_and_disable_reject_unknown_destination() {
        let state = state_with_table();
        let req = json!({ "TableName": "t", "StreamArn": ARN });
        assert_eq!(
            update_kinesis_streaming_destination(&state, &req, &ctx())
                .unwrap_err()
                .code,
            "ValidationException"
        );
        assert_eq!(
            disable_kinesis_streaming_destination(&state, &req, &ctx())
                .unwrap_err()
                .code,
            "ValidationException"
        );
    }
}
