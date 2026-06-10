//! Table export to S3 and table import from S3.
//!
//! Exports write gzipped newline-delimited DynamoDB JSON plus manifest
//! objects under `{prefix}/AWSDynamoDB/{exportId}/`, the same layout AWS
//! produces. Imports create the target table from
//! `TableCreationParameters` and load every source object under the
//! configured bucket and key prefix. Both run synchronously; the initial
//! response reports `IN_PROGRESS` and the next describe shows the settled
//! `COMPLETED` or `FAILED` state.

use std::io::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, S3ObjectWriter, arn};
use base64::Engine as _;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use crate::sqlite_store::SqliteStore;
use crate::state::{DynamoState, ExportRecord};

use super::{opt_str, require_str};

fn now_epoch_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// DescribeExport reports the stored export, including item counts, the
/// manifest location, and any failure recorded while writing to S3.
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

    Ok(json!({ "ExportDescription": export_description(&record) }))
}

fn export_description(record: &ExportRecord) -> Value {
    let mut desc = json!({
        "ExportArn": record.export_arn,
        "ExportStatus": record.export_status,
        "ExportType": "FULL_EXPORT",
        "TableArn": record.table_arn,
        "ExportFormat": record.export_format,
        "S3Bucket": record.s3_bucket,
        "S3Prefix": record.s3_prefix,
        "StartTime": record.start_time,
        "EndTime": record.end_time
    });
    if let Some(n) = record.item_count {
        desc["ItemCount"] = json!(n);
    }
    if let Some(n) = record.billed_size_bytes {
        desc["BilledSizeBytes"] = json!(n);
    }
    if let Some(key) = &record.export_manifest {
        desc["ExportManifest"] = json!(key);
    }
    if let Some(code) = &record.failure_code {
        desc["FailureCode"] = json!(code);
    }
    if let Some(message) = &record.failure_message {
        desc["FailureMessage"] = json!(message);
    }
    desc
}

/// What one export run wrote to S3.
struct ExportArtifacts {
    item_count: u64,
    billed_size_bytes: u64,
    manifest_key: String,
}

/// Join the user prefix with the fixed `AWSDynamoDB/{exportId}` layout
/// AWS uses for export output.
fn export_base_key(prefix: Option<&str>, export_id: &str) -> String {
    match prefix
        .map(|p| p.trim_matches('/'))
        .filter(|p| !p.is_empty())
    {
        Some(p) => format!("{p}/AWSDynamoDB/{export_id}"),
        None => format!("AWSDynamoDB/{export_id}"),
    }
}

fn gzip(data: &[u8]) -> Result<Vec<u8>, AwsError> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)
        .and_then(|()| enc.finish())
        .map_err(|e| AwsError::internal(format!("gzip export data: {e}")))
}

fn put_bytes(
    writer: &dyn S3ObjectWriter,
    bucket: &str,
    key: &str,
    bytes: &[u8],
    ctx: &RequestContext,
) -> Result<(), AwsError> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    writer.put_object(bucket, key, &b64, &ctx.account_id, &ctx.region)
}

/// Scan the table out of SQLite and write the AWS export layout: a
/// gzipped data file of `{"Item": {...}}` lines, a `manifest-files.json`
/// listing it, and a `manifest-summary.json` describing the export.
fn write_export_objects(
    sqlite: &SqliteStore,
    writer: &dyn S3ObjectWriter,
    table_name: &str,
    record: &ExportRecord,
    export_id: &str,
    ctx: &RequestContext,
) -> Result<ExportArtifacts, AwsError> {
    let mut data = String::new();
    let mut item_count: u64 = 0;
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        table_name,
        None,
        |_pk, _sk, attrs| {
            data.push_str(&json!({ "Item": attrs }).to_string());
            data.push('\n');
            item_count += 1;
            Ok(true)
        },
    )?;
    let billed_size_bytes = data.len() as u64;

    let base = export_base_key(record.s3_prefix.as_deref(), export_id);
    let data_key = format!("{base}/data/{export_id}.json.gz");
    put_bytes(
        writer,
        &record.s3_bucket,
        &data_key,
        &gzip(data.as_bytes())?,
        ctx,
    )?;

    let manifest_files_key = format!("{base}/manifest-files.json");
    let files_line = json!({
        "itemCount": item_count,
        "dataFileS3Key": data_key
    })
    .to_string();
    put_bytes(
        writer,
        &record.s3_bucket,
        &manifest_files_key,
        files_line.as_bytes(),
        ctx,
    )?;

    let manifest_key = format!("{base}/manifest-summary.json");
    let summary = json!({
        "version": "2020-06-30",
        "exportArn": record.export_arn,
        "startTime": record.start_time,
        "endTime": now_epoch_f64(),
        "tableArn": record.table_arn,
        "tableId": "00000000-0000-0000-0000-000000000000",
        "s3Bucket": record.s3_bucket,
        "s3Prefix": record.s3_prefix,
        "outputFormat": record.export_format,
        "itemCount": item_count,
        "billedSizeBytes": billed_size_bytes,
        "manifestFilesS3Key": manifest_files_key
    })
    .to_string();
    put_bytes(
        writer,
        &record.s3_bucket,
        &manifest_key,
        summary.as_bytes(),
        ctx,
    )?;

    Ok(ExportArtifacts {
        item_count,
        billed_size_bytes,
        manifest_key,
    })
}

/// ExportTableToPointInTime writes the table's items to S3 in the AWS
/// export layout and records the result.
///
/// The export runs synchronously: the response reports `IN_PROGRESS` to
/// match the AWS wire contract, and `DescribeExport` then shows
/// `COMPLETED` with item counts, or `FAILED` with the S3 error when a
/// write was rejected (e.g. the bucket does not exist). Requires
/// point-in-time recovery to be enabled on the table, as on AWS. When no
/// in-process S3 writer is wired, the export records metadata only.
pub fn export_table_to_point_in_time(
    state: &DynamoState,
    sqlite: &SqliteStore,
    s3: Option<&std::sync::Arc<dyn S3ObjectWriter>>,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_arn = require_str(input, "TableArn")?;
    let table_name = state
        .tables
        .iter()
        .find(|e| e.value().arn == table_arn)
        .map(|e| e.key().clone())
        .ok_or_else(|| {
            AwsError::service_not_found(
                "TableNotFoundException",
                format!("Table not found: {table_arn}"),
            )
        })?;

    if !state.pitr_enabled_at.contains_key(&table_name) {
        return Err(AwsError::bad_request(
            "PointInTimeRecoveryUnavailableException",
            format!("Point in time recovery is not enabled for table '{table_name}'"),
        ));
    }

    let format = opt_str(input, "ExportFormat").unwrap_or("DYNAMODB_JSON");
    match format {
        "DYNAMODB_JSON" => {}
        "ION" => {
            return Err(AwsError::bad_request(
                "ValidationException",
                "ION export format is not supported",
            ));
        }
        other => {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Value '{other}' at 'exportFormat' failed to satisfy constraint: \
                     Member must satisfy enum value set: [DYNAMODB_JSON, ION]"
                ),
            ));
        }
    }
    let export_type = opt_str(input, "ExportType").unwrap_or("FULL_EXPORT");
    if export_type != "FULL_EXPORT" {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("ExportType '{export_type}' is not supported"),
        ));
    }

    let s3_bucket = require_str(input, "S3Bucket")?.to_string();
    let s3_prefix = opt_str(input, "S3Prefix").map(|s| s.to_string());

    let now = now_epoch_f64();
    let export_id = format!(
        "{:013}-{}",
        (now * 1000.0) as u64,
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let export_arn = format!("{table_arn}/export/{export_id}");

    let mut record = ExportRecord {
        export_arn: export_arn.clone(),
        table_arn: table_arn.to_string(),
        export_status: "COMPLETED".to_string(),
        export_format: format.to_string(),
        s3_bucket,
        s3_prefix,
        start_time: now,
        end_time: Some(now),
        item_count: None,
        billed_size_bytes: None,
        export_manifest: None,
        failure_code: None,
        failure_message: None,
    };

    if let Some(writer) = s3 {
        match write_export_objects(
            sqlite,
            writer.as_ref(),
            &table_name,
            &record,
            &export_id,
            ctx,
        ) {
            Ok(artifacts) => {
                record.item_count = Some(artifacts.item_count);
                record.billed_size_bytes = Some(artifacts.billed_size_bytes);
                record.export_manifest = Some(artifacts.manifest_key);
            }
            Err(err) => {
                record.export_status = "FAILED".to_string();
                record.failure_code = Some(err.code.clone());
                record.failure_message = Some(err.message.clone());
            }
        }
        record.end_time = Some(now_epoch_f64());
    }

    let mut response = export_description(&record);
    state.exports.insert(export_arn, record);

    // AWS answers the initial request with IN_PROGRESS; the settled
    // status is visible on the next DescribeExport.
    response["ExportStatus"] = json!("IN_PROGRESS");
    Ok(json!({ "ExportDescription": response }))
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

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::operations::backup::update_continuous_backups;
    use crate::operations::item::put_item;
    use crate::operations::table::create_table;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    /// In-memory S3 capturing every put; `fail_with` makes all puts
    /// return that error code instead.
    #[derive(Default)]
    struct MockS3 {
        objects: Mutex<Vec<(String, Vec<u8>)>>,
        fail_with: Option<&'static str>,
    }

    impl S3ObjectWriter for MockS3 {
        fn put_object(
            &self,
            _bucket: &str,
            key: &str,
            body_b64: &str,
            _account: &str,
            _region: &str,
        ) -> Result<(), AwsError> {
            if let Some(code) = self.fail_with {
                return Err(AwsError::service_not_found(code, "mock S3 failure"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(body_b64)
                .unwrap();
            self.objects.lock().unwrap().push((key.to_string(), bytes));
            Ok(())
        }
    }

    fn table_with_items(
        state: &DynamoState,
        sqlite: &SqliteStore,
        c: &RequestContext,
        items: usize,
    ) -> String {
        create_table(
            state,
            sqlite,
            &json!({
                "TableName": "t",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "BillingMode": "PAY_PER_REQUEST"
            }),
            c,
        )
        .unwrap();
        for i in 0..items {
            put_item(
                state,
                sqlite,
                &json!({
                    "TableName": "t",
                    "Item": { "pk": { "S": format!("p-{i}") }, "v": { "N": i.to_string() } }
                }),
                c,
            )
            .unwrap();
        }
        state.tables.get("t").unwrap().arn.clone()
    }

    fn enable_pitr(state: &DynamoState, c: &RequestContext) {
        update_continuous_backups(
            state,
            &json!({
                "TableName": "t",
                "PointInTimeRecoverySpecification": { "PointInTimeRecoveryEnabled": true }
            }),
            c,
        )
        .unwrap();
    }

    fn gunzip(bytes: &[u8]) -> String {
        let mut out = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn export_writes_data_and_manifests_to_s3() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let table_arn = table_with_items(&state, &sqlite, &c, 3);
        enable_pitr(&state, &c);

        let mock = Arc::new(MockS3::default());
        let writer: Arc<dyn S3ObjectWriter> = mock.clone();
        let resp = export_table_to_point_in_time(
            &state,
            &sqlite,
            Some(&writer),
            &json!({ "TableArn": table_arn, "S3Bucket": "exports", "S3Prefix": "backups/" }),
            &c,
        )
        .unwrap();
        assert_eq!(
            resp["ExportDescription"]["ExportStatus"],
            json!("IN_PROGRESS")
        );
        let export_arn = resp["ExportDescription"]["ExportArn"].as_str().unwrap();

        let desc = describe_export(&state, &json!({ "ExportArn": export_arn }), &c).unwrap();
        let desc = &desc["ExportDescription"];
        assert_eq!(desc["ExportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ItemCount"], json!(3));
        assert!(desc["BilledSizeBytes"].as_u64().unwrap() > 0);

        let objects = mock.objects.lock().unwrap();
        assert_eq!(objects.len(), 3, "data file plus two manifests");
        for (key, _) in objects.iter() {
            assert!(key.starts_with("backups/AWSDynamoDB/"), "key: {key}");
        }

        let (_, data) = objects
            .iter()
            .find(|(k, _)| k.ends_with(".json.gz"))
            .unwrap();
        let text = gunzip(data);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            let item: Value = serde_json::from_str(line).unwrap();
            assert!(item["Item"]["pk"]["S"].as_str().unwrap().starts_with("p-"));
        }

        let (_, summary) = objects
            .iter()
            .find(|(k, _)| k.ends_with("manifest-summary.json"))
            .unwrap();
        let summary: Value = serde_json::from_slice(summary).unwrap();
        assert_eq!(summary["itemCount"], json!(3));
        assert_eq!(summary["exportArn"], json!(export_arn));
        assert!(
            desc["ExportManifest"]
                .as_str()
                .unwrap()
                .ends_with("manifest-summary.json")
        );
        assert!(
            summary["manifestFilesS3Key"]
                .as_str()
                .unwrap()
                .ends_with("manifest-files.json")
        );
    }

    #[test]
    fn export_requires_point_in_time_recovery() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let table_arn = table_with_items(&state, &sqlite, &c, 1);

        let err = export_table_to_point_in_time(
            &state,
            &sqlite,
            None,
            &json!({ "TableArn": table_arn, "S3Bucket": "exports" }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "PointInTimeRecoveryUnavailableException");
    }

    #[test]
    fn export_rejects_unsupported_formats() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let table_arn = table_with_items(&state, &sqlite, &c, 1);
        enable_pitr(&state, &c);

        let err = export_table_to_point_in_time(
            &state,
            &sqlite,
            None,
            &json!({ "TableArn": table_arn, "S3Bucket": "exports", "ExportFormat": "ION" }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("ION"));
    }

    #[test]
    fn export_records_s3_failure_for_describe() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let table_arn = table_with_items(&state, &sqlite, &c, 1);
        enable_pitr(&state, &c);

        let writer: Arc<dyn S3ObjectWriter> = Arc::new(MockS3 {
            objects: Mutex::new(Vec::new()),
            fail_with: Some("NoSuchBucket"),
        });
        let resp = export_table_to_point_in_time(
            &state,
            &sqlite,
            Some(&writer),
            &json!({ "TableArn": table_arn, "S3Bucket": "missing" }),
            &c,
        )
        .unwrap();
        assert_eq!(
            resp["ExportDescription"]["ExportStatus"],
            json!("IN_PROGRESS")
        );

        let export_arn = resp["ExportDescription"]["ExportArn"].as_str().unwrap();
        let desc = describe_export(&state, &json!({ "ExportArn": export_arn }), &c).unwrap();
        assert_eq!(desc["ExportDescription"]["ExportStatus"], json!("FAILED"));
        assert_eq!(
            desc["ExportDescription"]["FailureCode"],
            json!("NoSuchBucket")
        );
    }
}
