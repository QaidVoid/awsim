//! Table export to S3 and table import from S3.

use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::DynamoState;

use super::{opt_str, require_str};

fn now_epoch_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn describe_export(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let export_arn = require_str(input, "ExportArn")?;
    let record = state.exports.get(export_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "ExportNotFoundException",
            format!("Export not found: {export_arn}"),
        )
    })?;

    Ok(json!({
        "ExportDescription": {
            "ExportArn": record.export_arn,
            "ExportStatus": record.export_status,
            "TableArn": record.table_arn,
            "ExportFormat": record.export_format,
            "S3Bucket": record.s3_bucket,
            "S3Prefix": record.s3_prefix,
            "StartTime": record.start_time,
            "EndTime": record.end_time
        }
    }))
}

pub fn export_table_to_point_in_time(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn = require_str(input, "TableArn")?;
    let exists = state.tables.iter().any(|e| e.value().arn == table_arn);
    if !exists {
        return Err(AwsError::service_not_found(
            "TableNotFoundException",
            format!("Table not found: {table_arn}"),
        ));
    }

    let s3_bucket = require_str(input, "S3Bucket")?.to_string();
    let s3_prefix = opt_str(input, "S3Prefix").map(|s| s.to_string());
    let format = opt_str(input, "ExportFormat")
        .unwrap_or("DYNAMODB_JSON")
        .to_string();

    let now = now_epoch_f64();
    let export_arn = format!("{}/export/{:016.0}", table_arn, now);
    let _ = ctx;

    let record = crate::state::ExportRecord {
        export_arn: export_arn.clone(),
        table_arn: table_arn.to_string(),
        export_status: "COMPLETED".to_string(),
        export_format: format.clone(),
        s3_bucket: s3_bucket.clone(),
        s3_prefix: s3_prefix.clone(),
        start_time: now,
        end_time: Some(now),
    };

    state.exports.insert(export_arn.clone(), record);

    Ok(json!({
        "ExportDescription": {
            "ExportArn": export_arn,
            "ExportStatus": "IN_PROGRESS",
            "TableArn": table_arn,
            "ExportFormat": format,
            "S3Bucket": s3_bucket,
            "S3Prefix": s3_prefix,
            "StartTime": now
        }
    }))
}

pub fn list_exports(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn_filter = opt_str(input, "TableArn");

    let summaries: Vec<Value> = state
        .exports
        .iter()
        .filter(|e| {
            table_arn_filter
                .map(|arn| e.value().table_arn == arn)
                .unwrap_or(true)
        })
        .map(|e| {
            json!({
                "ExportArn": e.value().export_arn,
                "ExportStatus": e.value().export_status,
                "ExportType": "FULL_EXPORT"
            })
        })
        .collect();

    Ok(json!({ "ExportSummaries": summaries }))
}

// ─── Import ───────────────────────────────────────────────────────────────────

pub fn describe_import(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let import_arn = require_str(input, "ImportArn")?;
    let record = state.imports.get(import_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "ImportNotFoundException",
            format!("Import not found: {import_arn}"),
        )
    })?;

    Ok(json!({
        "ImportTableDescription": {
            "ImportArn": record.import_arn,
            "ImportStatus": record.import_status,
            "TableArn": record.table_arn,
            "TableId": "00000000-0000-0000-0000-000000000000",
            "InputFormat": record.input_format,
            "S3BucketSource": {
                "S3Bucket": record.s3_bucket
            },
            "StartTime": record.start_time,
            "EndTime": record.end_time
        }
    }))
}

pub fn import_table(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let params = input.get("TableCreationParameters").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "TableCreationParameters is required")
    })?;
    let table_name = params
        .get("TableName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "TableName is required"))?;

    let s3_source = input.get("S3BucketSource").ok_or_else(|| {
        AwsError::bad_request("ValidationException", "S3BucketSource is required")
    })?;
    let s3_bucket = s3_source
        .get("S3Bucket")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let input_format = opt_str(input, "InputFormat")
        .unwrap_or("DYNAMODB_JSON")
        .to_string();

    let now = now_epoch_f64();
    let table_arn = arn::build(ctx, "dynamodb", format!("table/{table_name}"));
    let import_arn = arn::build(
        ctx,
        "dynamodb",
        format!("table/{table_name}/import/{now:016.0}"),
    );

    let record = crate::state::ImportRecord {
        import_arn: import_arn.clone(),
        table_arn: table_arn.clone(),
        table_name: table_name.to_string(),
        import_status: "IN_PROGRESS".to_string(),
        input_format: input_format.clone(),
        s3_bucket: s3_bucket.clone(),
        start_time: now,
        end_time: None,
    };

    state.imports.insert(import_arn.clone(), record);

    Ok(json!({
        "ImportTableDescription": {
            "ImportArn": import_arn,
            "ImportStatus": "IN_PROGRESS",
            "TableArn": table_arn,
            "TableId": "00000000-0000-0000-0000-000000000000",
            "InputFormat": input_format,
            "S3BucketSource": {
                "S3Bucket": s3_bucket
            },
            "StartTime": now
        }
    }))
}

pub fn list_imports(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn_filter = opt_str(input, "TableArn");

    let summaries: Vec<Value> = state
        .imports
        .iter()
        .filter(|e| {
            table_arn_filter
                .map(|arn| e.value().table_arn == arn)
                .unwrap_or(true)
        })
        .map(|e| {
            json!({
                "ImportArn": e.value().import_arn,
                "ImportStatus": e.value().import_status,
                "TableArn": e.value().table_arn,
                "S3BucketSource": {
                    "S3Bucket": e.value().s3_bucket
                },
                "InputFormat": e.value().input_format,
                "StartTime": e.value().start_time,
                "EndTime": e.value().end_time
            })
        })
        .collect();

    Ok(json!({ "ImportSummaryList": summaries }))
}
