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

use awsim_core::{AwsError, RequestContext, S3ObjectReader, S3ObjectWriter, arn};
use base64::Engine as _;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use crate::sqlite_store::SqliteStore;
use crate::state::{DynamoState, ExportRecord, ImportRecord};

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

/// DescribeImport reports the stored import, including item counts and
/// any failure recorded while reading from S3.
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

    Ok(json!({ "ImportTableDescription": import_description(&record) }))
}

fn import_description(record: &ImportRecord) -> Value {
    let mut source = json!({ "S3Bucket": record.s3_bucket });
    if let Some(prefix) = &record.s3_key_prefix {
        source["S3KeyPrefix"] = json!(prefix);
    }
    let mut desc = json!({
        "ImportArn": record.import_arn,
        "ImportStatus": record.import_status,
        "TableArn": record.table_arn,
        "TableId": "00000000-0000-0000-0000-000000000000",
        "InputFormat": record.input_format,
        "S3BucketSource": source,
        "StartTime": record.start_time,
        "EndTime": record.end_time,
        "ProcessedItemCount": record.processed_item_count,
        "ImportedItemCount": record.imported_item_count,
        "ProcessedSizeBytes": record.processed_size_bytes,
        "ErrorCount": record.error_count
    });
    if let Some(compression) = &record.input_compression_type {
        desc["InputCompressionType"] = json!(compression);
    }
    if let Some(params) = &record.table_creation_parameters {
        desc["TableCreationParameters"] = params.clone();
    }
    if let Some(code) = &record.failure_code {
        desc["FailureCode"] = json!(code);
    }
    if let Some(message) = &record.failure_message {
        desc["FailureMessage"] = json!(message);
    }
    desc
}

/// Counters accumulated while loading source objects.
#[derive(Default)]
struct ImportStats {
    imported: u64,
    processed: u64,
    size_bytes: u64,
    errors: u64,
}

/// Per-file CSV settings resolved from `InputFormatOptions.Csv`.
struct CsvOptions {
    delimiter: char,
    header_list: Option<Vec<String>>,
}

fn csv_options(input: &Value) -> Result<CsvOptions, AwsError> {
    let csv = input
        .get("InputFormatOptions")
        .and_then(|o| o.get("Csv"))
        .cloned()
        .unwrap_or(Value::Null);
    let delimiter = match csv.get("Delimiter").and_then(|v| v.as_str()) {
        None => ',',
        Some(d) if d.chars().count() == 1 => d.chars().next().unwrap_or(','),
        Some(d) => {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!("Delimiter '{d}' must be a single character"),
            ));
        }
    };
    let header_list = csv.get("HeaderList").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|h| h.as_str().map(String::from))
            .collect()
    });
    Ok(CsvOptions {
        delimiter,
        header_list,
    })
}

/// Split one CSV record into fields, honoring double-quoted fields with
/// `""` escapes.
fn parse_csv_record(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' && current.is_empty() {
            in_quotes = true;
        } else if c == delimiter {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    fields.push(current);
    fields
}

/// Turn one CSV record into a wire-format item. Key and index attributes
/// take their declared type from `attr_types`; every other column is a
/// DynamoDB string, matching the AWS CSV import contract. Empty fields
/// are omitted.
fn csv_record_to_item(
    headers: &[String],
    fields: &[String],
    attr_types: &std::collections::HashMap<String, String>,
) -> Value {
    let mut item = serde_json::Map::new();
    for (header, field) in headers.iter().zip(fields) {
        if field.is_empty() {
            continue;
        }
        let attr_type = attr_types.get(header).map(String::as_str).unwrap_or("S");
        item.insert(header.clone(), json!({ attr_type: field }));
    }
    Value::Object(item)
}

fn gunzip_text(bytes: &[u8]) -> Result<String, AwsError> {
    use std::io::Read as _;
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .map_err(|e| AwsError::internal(format!("decompress import object: {e}")))?;
    Ok(text)
}

/// Write one wire-format item into the table. Returns false (counted as
/// an item error) when the item is malformed or missing key attributes;
/// propagates real storage failures.
fn put_import_item(
    sqlite: &SqliteStore,
    table: &crate::state::Table,
    item_val: &Value,
    ctx: &RequestContext,
) -> Result<bool, AwsError> {
    let Some(item) = crate::keys::storage_value_to_item(item_val.clone()) else {
        return Ok(false);
    };
    let Some(keys) = crate::keys::extract_item_keys(table, &item) else {
        return Ok(false);
    };
    sqlite.put_item(
        &ctx.account_id,
        &ctx.region,
        &table.name,
        &keys.pk,
        &keys.sk,
        item_val,
        &keys.gsi,
    )?;
    Ok(true)
}

/// Read every source object under the bucket and key prefix and load its
/// items into the freshly created table.
#[allow(clippy::too_many_arguments)]
fn load_import_objects(
    state: &DynamoState,
    sqlite: &SqliteStore,
    reader: &dyn S3ObjectReader,
    table_name: &str,
    bucket: &str,
    prefix: &str,
    format: &str,
    gzipped: bool,
    csv: &CsvOptions,
    ctx: &RequestContext,
) -> Result<ImportStats, AwsError> {
    let table = state
        .tables
        .get(table_name)
        .map(|t| t.value().clone())
        .ok_or_else(|| AwsError::internal("import target table disappeared"))?;
    let attr_types: std::collections::HashMap<String, String> = table
        .attribute_definitions
        .iter()
        .map(|d| (d.attribute_name.clone(), d.attribute_type.clone()))
        .collect();

    let mut stats = ImportStats::default();
    for key in reader.list_objects(bucket, prefix, &ctx.account_id, &ctx.region)? {
        let bytes = reader.get_object(bucket, &key, &ctx.account_id, &ctx.region)?;
        stats.size_bytes += bytes.len() as u64;
        let text = if gzipped {
            gunzip_text(&bytes)?
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };

        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let headers: Vec<String> = if format == "CSV" {
            match &csv.header_list {
                Some(list) => list.clone(),
                // Without an explicit HeaderList the first record of each
                // file is its header row.
                None => match lines.next() {
                    Some(line) => parse_csv_record(line, csv.delimiter)
                        .into_iter()
                        .map(|h| h.trim().to_string())
                        .collect(),
                    None => continue,
                },
            }
        } else {
            Vec::new()
        };

        for line in lines {
            stats.processed += 1;
            let item_val = if format == "CSV" {
                let fields = parse_csv_record(line, csv.delimiter);
                csv_record_to_item(&headers, &fields, &attr_types)
            } else {
                match serde_json::from_str::<Value>(line) {
                    // Lines are `{"Item": {...}}` in AWS export output;
                    // accept a bare item object as well.
                    Ok(v) => v.get("Item").cloned().unwrap_or(v),
                    Err(_) => {
                        stats.errors += 1;
                        continue;
                    }
                }
            };
            if put_import_item(sqlite, &table, &item_val, ctx)? {
                stats.imported += 1;
            } else {
                stats.errors += 1;
            }
        }
    }
    Ok(stats)
}

/// ImportTable creates the target table from `TableCreationParameters`
/// and loads every source object under the configured S3 bucket and key
/// prefix.
///
/// The import runs synchronously: the response reports `IN_PROGRESS` to
/// match the AWS wire contract, and `DescribeImport` then shows
/// `COMPLETED` with item counts (malformed source items are skipped and
/// surface in `ErrorCount`), or `FAILED` with the S3 error when the
/// source could not be read. Supports `DYNAMODB_JSON` and `CSV` input in
/// plain or GZIP compression. When no in-process S3 reader is wired, the
/// import creates the table without loading items.
pub fn import_table(
    state: &DynamoState,
    sqlite: &SqliteStore,
    s3: Option<&std::sync::Arc<dyn S3ObjectReader>>,
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
        .filter(|b| !b.is_empty())
        .ok_or_else(|| AwsError::bad_request("ValidationException", "S3Bucket is required"))?
        .to_string();
    let s3_key_prefix = s3_source
        .get("S3KeyPrefix")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input_format = opt_str(input, "InputFormat").unwrap_or("DYNAMODB_JSON");
    match input_format {
        "DYNAMODB_JSON" | "CSV" => {}
        "ION" => {
            return Err(AwsError::bad_request(
                "ValidationException",
                "ION input format is not supported",
            ));
        }
        other => {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Value '{other}' at 'inputFormat' failed to satisfy constraint: \
                     Member must satisfy enum value set: [CSV, DYNAMODB_JSON, ION]"
                ),
            ));
        }
    }
    let compression = opt_str(input, "InputCompressionType").unwrap_or("NONE");
    let gzipped = match compression {
        "NONE" => false,
        "GZIP" => true,
        "ZSTD" => {
            return Err(AwsError::bad_request(
                "ValidationException",
                "ZSTD input compression is not supported",
            ));
        }
        other => {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "Value '{other}' at 'inputCompressionType' failed to satisfy constraint: \
                     Member must satisfy enum value set: [GZIP, ZSTD, NONE]"
                ),
            ));
        }
    };
    let csv = csv_options(input)?;

    // Create the target table first; a name collision surfaces as the
    // usual ResourceInUseException before any data moves.
    crate::operations::table::create_table(state, sqlite, params, ctx)?;

    let now = now_epoch_f64();
    let table_arn = arn::build(ctx, "dynamodb", format!("table/{table_name}"));
    let import_arn = arn::build(
        ctx,
        "dynamodb",
        format!("table/{table_name}/import/{now:016.0}"),
    );

    let mut record = ImportRecord {
        import_arn: import_arn.clone(),
        table_arn,
        table_name: table_name.to_string(),
        import_status: "COMPLETED".to_string(),
        input_format: input_format.to_string(),
        s3_bucket: s3_bucket.clone(),
        start_time: now,
        end_time: None,
        s3_key_prefix: s3_key_prefix.clone(),
        input_compression_type: Some(compression.to_string()),
        imported_item_count: 0,
        processed_item_count: 0,
        processed_size_bytes: 0,
        error_count: 0,
        failure_code: None,
        failure_message: None,
        table_creation_parameters: Some(params.clone()),
    };

    if let Some(reader) = s3 {
        match load_import_objects(
            state,
            sqlite,
            reader.as_ref(),
            table_name,
            &s3_bucket,
            s3_key_prefix.as_deref().unwrap_or(""),
            input_format,
            gzipped,
            &csv,
            ctx,
        ) {
            Ok(stats) => {
                record.imported_item_count = stats.imported;
                record.processed_item_count = stats.processed;
                record.processed_size_bytes = stats.size_bytes;
                record.error_count = stats.errors;
            }
            Err(err) => {
                record.import_status = "FAILED".to_string();
                record.failure_code = Some(err.code.clone());
                record.failure_message = Some(err.message.clone());
            }
        }
    }
    record.end_time = Some(now_epoch_f64());

    let mut response = import_description(&record);
    state.imports.insert(import_arn, record);

    // AWS answers the initial request with IN_PROGRESS; the settled
    // status is visible on the next DescribeImport.
    response["ImportStatus"] = json!("IN_PROGRESS");
    Ok(json!({ "ImportTableDescription": response }))
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

    /// In-memory S3 source for imports, keyed by object key.
    #[derive(Default)]
    struct MockSource {
        objects: std::collections::BTreeMap<String, Vec<u8>>,
    }

    impl S3ObjectReader for MockSource {
        fn get_object(
            &self,
            _bucket: &str,
            key: &str,
            _account: &str,
            _region: &str,
        ) -> Result<Vec<u8>, AwsError> {
            self.objects.get(key).cloned().ok_or_else(|| {
                AwsError::service_not_found("NoSuchKey", format!("missing key: {key}"))
            })
        }

        fn list_objects(
            &self,
            _bucket: &str,
            prefix: &str,
            _account: &str,
            _region: &str,
        ) -> Result<Vec<String>, AwsError> {
            Ok(self
                .objects
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    fn import_input(format: &str) -> Value {
        json!({
            "TableCreationParameters": {
                "TableName": "imported",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "BillingMode": "PAY_PER_REQUEST"
            },
            "S3BucketSource": { "S3Bucket": "src", "S3KeyPrefix": "seed/" },
            "InputFormat": format
        })
    }

    fn run_import(
        state: &DynamoState,
        sqlite: &SqliteStore,
        source: MockSource,
        input: &Value,
        c: &RequestContext,
    ) -> Value {
        let reader: Arc<dyn S3ObjectReader> = Arc::new(source);
        let resp = import_table(state, sqlite, Some(&reader), input, c).unwrap();
        assert_eq!(
            resp["ImportTableDescription"]["ImportStatus"],
            json!("IN_PROGRESS")
        );
        let import_arn = resp["ImportTableDescription"]["ImportArn"]
            .as_str()
            .unwrap();
        describe_import(state, &json!({ "ImportArn": import_arn }), c).unwrap()
            ["ImportTableDescription"]
            .clone()
    }

    #[test]
    fn import_dynamodb_json_creates_table_and_loads_items() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let mut source = MockSource::default();
        source.objects.insert(
            "seed/part-1.json".into(),
            b"{\"Item\":{\"pk\":{\"S\":\"a\"},\"v\":{\"N\":\"1\"}}}\n{\"Item\":{\"pk\":{\"S\":\"b\"}}}\n"
                .to_vec(),
        );
        source.objects.insert(
            "seed/part-2.json".into(),
            b"{\"Item\":{\"pk\":{\"S\":\"c\"}}}\n".to_vec(),
        );
        source
            .objects
            .insert("other/ignored.json".into(), b"{}".to_vec());

        let desc = run_import(&state, &sqlite, source, &import_input("DYNAMODB_JSON"), &c);
        assert_eq!(desc["ImportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ImportedItemCount"], json!(3));
        assert_eq!(desc["ProcessedItemCount"], json!(3));
        assert_eq!(desc["ErrorCount"], json!(0));
        assert!(desc["ProcessedSizeBytes"].as_u64().unwrap() > 0);

        let got = crate::operations::item::get_item(
            &state,
            &sqlite,
            &json!({ "TableName": "imported", "Key": { "pk": { "S": "a" } } }),
            &c,
        )
        .unwrap();
        assert_eq!(got["Item"]["v"]["N"], json!("1"));
    }

    #[test]
    fn import_reads_gzip_compressed_objects() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let mut source = MockSource::default();
        source.objects.insert(
            "seed/part-1.json.gz".into(),
            gzip(b"{\"Item\":{\"pk\":{\"S\":\"a\"}}}\n").unwrap(),
        );

        let mut input = import_input("DYNAMODB_JSON");
        input["InputCompressionType"] = json!("GZIP");
        let desc = run_import(&state, &sqlite, source, &input, &c);
        assert_eq!(desc["ImportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ImportedItemCount"], json!(1));
    }

    #[test]
    fn import_csv_types_key_columns_and_strings_the_rest() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let mut source = MockSource::default();
        source.objects.insert(
            "seed/data.csv".into(),
            b"id,name,score\n1,\"Ada, B.\",10\n2,Grace,\n".to_vec(),
        );

        let mut input = import_input("CSV");
        input["TableCreationParameters"]["KeySchema"] =
            json!([{ "AttributeName": "id", "KeyType": "HASH" }]);
        input["TableCreationParameters"]["AttributeDefinitions"] =
            json!([{ "AttributeName": "id", "AttributeType": "N" }]);
        let desc = run_import(&state, &sqlite, source, &input, &c);
        assert_eq!(desc["ImportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ImportedItemCount"], json!(2));

        let got = crate::operations::item::get_item(
            &state,
            &sqlite,
            &json!({ "TableName": "imported", "Key": { "id": { "N": "1" } } }),
            &c,
        )
        .unwrap();
        assert_eq!(got["Item"]["name"]["S"], json!("Ada, B."));
        assert_eq!(got["Item"]["score"]["S"], json!("10"));
        let sparse = crate::operations::item::get_item(
            &state,
            &sqlite,
            &json!({ "TableName": "imported", "Key": { "id": { "N": "2" } } }),
            &c,
        )
        .unwrap();
        assert!(sparse["Item"].get("score").is_none(), "empty field omitted");
    }

    #[test]
    fn import_counts_bad_source_items_without_failing() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let mut source = MockSource::default();
        source.objects.insert(
            "seed/part-1.json".into(),
            b"{\"Item\":{\"pk\":{\"S\":\"a\"}}}\nnot json\n{\"Item\":{\"wrong\":{\"S\":\"x\"}}}\n"
                .to_vec(),
        );

        let desc = run_import(&state, &sqlite, source, &import_input("DYNAMODB_JSON"), &c);
        assert_eq!(desc["ImportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ImportedItemCount"], json!(1));
        assert_eq!(desc["ProcessedItemCount"], json!(3));
        assert_eq!(desc["ErrorCount"], json!(2));
    }

    #[test]
    fn import_rejects_unsupported_format_and_compression() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let err = import_table(&state, &sqlite, None, &import_input("ION"), &c).unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("ION"));

        let mut input = import_input("DYNAMODB_JSON");
        input["InputCompressionType"] = json!("ZSTD");
        let err = import_table(&state, &sqlite, None, &input, &c).unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("ZSTD"));
    }

    #[test]
    fn exported_data_files_round_trip_through_import() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let table_arn = table_with_items(&state, &sqlite, &c, 5);
        enable_pitr(&state, &c);

        let mock = Arc::new(MockS3::default());
        let writer: Arc<dyn S3ObjectWriter> = mock.clone();
        export_table_to_point_in_time(
            &state,
            &sqlite,
            Some(&writer),
            &json!({ "TableArn": table_arn, "S3Bucket": "exports" }),
            &c,
        )
        .unwrap();

        let mut source = MockSource::default();
        for (key, bytes) in mock.objects.lock().unwrap().iter() {
            if key.ends_with(".json.gz") {
                source.objects.insert(key.clone(), bytes.clone());
            }
        }
        assert_eq!(source.objects.len(), 1);

        let mut input = import_input("DYNAMODB_JSON");
        input["S3BucketSource"]["S3KeyPrefix"] = json!("AWSDynamoDB/");
        input["InputCompressionType"] = json!("GZIP");
        let desc = run_import(&state, &sqlite, source, &input, &c);
        assert_eq!(desc["ImportStatus"], json!("COMPLETED"));
        assert_eq!(desc["ImportedItemCount"], json!(5));
        assert_eq!(desc["ErrorCount"], json!(0));
    }

    #[test]
    fn import_into_existing_table_is_rejected() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        create_table(
            &state,
            &sqlite,
            &import_input("DYNAMODB_JSON")["TableCreationParameters"],
            &c,
        )
        .unwrap();

        let err =
            import_table(&state, &sqlite, None, &import_input("DYNAMODB_JSON"), &c).unwrap_err();
        assert_eq!(err.code, "ResourceInUseException");
    }
}
