//! Amazon QLDB emulator. Stores ledger metadata only — the journal/ION query
//! data plane is not implemented.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
    clamp_max_results_strict, paginate,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::debug;

#[derive(Debug, Default)]
pub struct QldbState {
    pub ledgers: DashMap<String, Ledger>,
    /// JournalKinesisStream records keyed by `StreamId` (UUID).
    pub kinesis_streams: DashMap<String, JournalKinesisStream>,
    /// JournalS3Export records keyed by `ExportId` (UUID).
    pub s3_exports: DashMap<String, JournalS3Export>,
    /// Per-ARN tag store for `stream` and `export` resources. Ledger
    /// tags continue to live on [`Ledger::tags`] for backwards
    /// compatibility with the original tag map.
    pub resource_tags: DashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalS3Export {
    pub export_id: String,
    pub ledger_name: String,
    pub role_arn: String,
    pub inclusive_start_time: f64,
    pub exclusive_end_time: f64,
    pub output_format: String,
    pub bucket: String,
    pub prefix: String,
    pub object_encryption_type: String,
    pub kms_key_arn: Option<String>,
    pub status: String,
    pub creation_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalKinesisStream {
    pub stream_id: String,
    pub ledger_name: String,
    pub stream_name: String,
    pub role_arn: String,
    pub kinesis_stream_arn: String,
    /// AWS accepts epoch seconds for the inclusive lower bound. The
    /// emulator stores it verbatim and replays it on Describe.
    pub inclusive_start_time: f64,
    pub exclusive_end_time: Option<f64>,
    pub aggregation_enabled: bool,
    pub creation_time: f64,
    pub status: String,
    pub error_cause: Option<String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub name: String,
    pub arn: String,
    pub state: String,
    pub creation_date_time: f64,
    pub permissions_mode: String,
    pub deletion_protection: bool,
    pub kms_key_arn: Option<String>,
    pub tags: HashMap<String, String>,
    /// `EncryptionStatus` field of the documented
    /// `EncryptionDescription` block. Persisted on the model so a
    /// future tick driver can flip it to `KMS_KEY_INACCESSIBLE` /
    /// `UPDATING` without rebuilding the structure on every read.
    #[serde(default = "default_encryption_status")]
    pub encryption_status: String,
    /// Epoch seconds when the KMS key first became inaccessible.
    /// `None` while the key is reachable; surfaced as JSON null on
    /// the API response.
    #[serde(default)]
    pub inaccessible_kms_key_date_time: Option<f64>,
}

fn default_encryption_status() -> String {
    "ENABLED".to_string()
}

/// AWS QLDB default quota: 5 ledgers per account per region. Above
/// this, `CreateLedger` returns `LimitExceededException`. The
/// emulator hardcodes the AWS default; an account-level config
/// surface can override this later if needed.
const LEDGER_QUOTA_PER_REGION: usize = 5;

#[derive(Debug, Serialize, Deserialize)]
pub struct QldbSnapshot {
    pub ledgers: Vec<Ledger>,
    #[serde(default)]
    pub kinesis_streams: Vec<JournalKinesisStream>,
    #[serde(default)]
    pub s3_exports: Vec<JournalS3Export>,
    #[serde(default)]
    pub resource_tags: HashMap<String, HashMap<String, String>>,
}

impl QldbState {
    pub fn to_snapshot(&self) -> QldbSnapshot {
        QldbSnapshot {
            ledgers: self.ledgers.iter().map(|e| e.value().clone()).collect(),
            kinesis_streams: self
                .kinesis_streams
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            s3_exports: self.s3_exports.iter().map(|e| e.value().clone()).collect(),
            resource_tags: self
                .resource_tags
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        }
    }
    pub fn restore_from_snapshot(&self, snap: QldbSnapshot) {
        self.ledgers.clear();
        for l in snap.ledgers {
            self.ledgers.insert(l.name.clone(), l);
        }
        self.kinesis_streams.clear();
        for s in snap.kinesis_streams {
            self.kinesis_streams.insert(s.stream_id.clone(), s);
        }
        self.s3_exports.clear();
        for ex in snap.s3_exports {
            self.s3_exports.insert(ex.export_id.clone(), ex);
        }
        self.resource_tags.clear();
        for (arn, tags) in snap.resource_tags {
            self.resource_tags.insert(arn, tags);
        }
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

fn ledger_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:qldb:{}:{}:ledger/{}",
        ctx.region, ctx.account_id, name
    )
}

fn stream_arn(ctx: &RequestContext, ledger: &str, stream_id: &str) -> String {
    format!(
        "arn:aws:qldb:{}:{}:stream/{ledger}/{stream_id}",
        ctx.region, ctx.account_id,
    )
}

fn stream_to_value(s: &JournalKinesisStream, ctx: &RequestContext) -> Value {
    let exclusive = match s.exclusive_end_time {
        Some(t) => json!(t),
        None => Value::Null,
    };
    json!({
        "LedgerName": s.ledger_name,
        "CreationTime": s.creation_time,
        "InclusiveStartTime": s.inclusive_start_time,
        "ExclusiveEndTime": exclusive,
        "RoleArn": s.role_arn,
        "StreamId": s.stream_id,
        "Arn": stream_arn(ctx, &s.ledger_name, &s.stream_id),
        "Status": s.status,
        "KinesisConfiguration": {
            "StreamArn": s.kinesis_stream_arn,
            "AggregationEnabled": s.aggregation_enabled,
        },
        "ErrorCause": s.error_cause,
        "StreamName": s.stream_name,
    })
}

/// Identifies a QLDB resource referenced by ARN. Only the kind and
/// the trailing identifier are needed to dispatch tag operations.
enum ResourceRef {
    Ledger(String),
    Stream(String),
    Export(String),
}

fn parse_resource_arn(arn: &str) -> Result<ResourceRef, AwsError> {
    let resource = arn.splitn(6, ':').nth(5).ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            format!("ResourceArn `{arn}` is malformed."),
        )
    })?;
    let (kind, tail) = resource.split_once('/').ok_or_else(|| {
        AwsError::bad_request(
            "BadRequestException",
            format!("ResourceArn `{arn}` is malformed."),
        )
    })?;
    match kind {
        "ledger" => Ok(ResourceRef::Ledger(tail.to_string())),
        "stream" => {
            let stream_id = tail.rsplit('/').next().unwrap_or(tail);
            Ok(ResourceRef::Stream(stream_id.to_string()))
        }
        "export" => {
            let export_id = tail.rsplit('/').next().unwrap_or(tail);
            Ok(ResourceRef::Export(export_id.to_string()))
        }
        _ => Err(AwsError::bad_request(
            "BadRequestException",
            format!("Resource kind `{kind}` is not a QLDB resource type."),
        )),
    }
}

fn apply_resource_tags(
    state: &QldbState,
    arn: &str,
    new_tags: HashMap<String, String>,
) -> Result<(), AwsError> {
    match parse_resource_arn(arn)? {
        ResourceRef::Ledger(name) => {
            let mut l = state.ledgers.get_mut(&name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Ledger {name} not found"),
                )
            })?;
            for (k, v) in new_tags {
                l.tags.insert(k, v);
            }
        }
        ResourceRef::Stream(id) => {
            if !state.kinesis_streams.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Stream {id} not found"),
                ));
            }
            let mut entry = state.resource_tags.entry(arn.to_string()).or_default();
            for (k, v) in new_tags {
                entry.insert(k, v);
            }
        }
        ResourceRef::Export(id) => {
            if !state.s3_exports.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Export {id} not found"),
                ));
            }
            let mut entry = state.resource_tags.entry(arn.to_string()).or_default();
            for (k, v) in new_tags {
                entry.insert(k, v);
            }
        }
    }
    Ok(())
}

fn remove_resource_tags(state: &QldbState, arn: &str, keys: &[String]) -> Result<(), AwsError> {
    match parse_resource_arn(arn)? {
        ResourceRef::Ledger(name) => {
            let mut l = state.ledgers.get_mut(&name).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Ledger {name} not found"),
                )
            })?;
            for k in keys {
                l.tags.remove(k);
            }
        }
        ResourceRef::Stream(id) => {
            if !state.kinesis_streams.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Stream {id} not found"),
                ));
            }
            if let Some(mut entry) = state.resource_tags.get_mut(arn) {
                for k in keys {
                    entry.remove(k);
                }
            }
        }
        ResourceRef::Export(id) => {
            if !state.s3_exports.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Export {id} not found"),
                ));
            }
            if let Some(mut entry) = state.resource_tags.get_mut(arn) {
                for k in keys {
                    entry.remove(k);
                }
            }
        }
    }
    Ok(())
}

fn read_resource_tags(state: &QldbState, arn: &str) -> Result<HashMap<String, String>, AwsError> {
    match parse_resource_arn(arn)? {
        ResourceRef::Ledger(name) => {
            state
                .ledgers
                .get(&name)
                .map(|l| l.tags.clone())
                .ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })
        }
        ResourceRef::Stream(id) => {
            if !state.kinesis_streams.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Stream {id} not found"),
                ));
            }
            Ok(state
                .resource_tags
                .get(arn)
                .map(|e| e.value().clone())
                .unwrap_or_default())
        }
        ResourceRef::Export(id) => {
            if !state.s3_exports.contains_key(&id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Export {id} not found"),
                ));
            }
            Ok(state
                .resource_tags
                .get(arn)
                .map(|e| e.value().clone())
                .unwrap_or_default())
        }
    }
}

fn export_to_value(ex: &JournalS3Export) -> Value {
    let mut encryption = json!({
        "ObjectEncryptionType": ex.object_encryption_type,
    });
    if let Some(arn) = &ex.kms_key_arn {
        encryption["KmsKeyArn"] = json!(arn);
    }
    json!({
        "LedgerName": ex.ledger_name,
        "ExportId": ex.export_id,
        "ExportCreationTime": ex.creation_time,
        "Status": ex.status,
        "InclusiveStartTime": ex.inclusive_start_time,
        "ExclusiveEndTime": ex.exclusive_end_time,
        "S3ExportConfiguration": {
            "Bucket": ex.bucket,
            "Prefix": ex.prefix,
            "EncryptionConfiguration": encryption,
        },
        "RoleArn": ex.role_arn,
        "OutputFormat": ex.output_format,
    })
}

fn ledger_to_value(l: &Ledger) -> Value {
    // `EncryptionDescription` documents three fields. The emulator
    // never simulates KMS key inaccessibility on its own, but the
    // status and inaccessible-date are persisted on the model so a
    // future tick driver can mutate them without changing the wire
    // shape.
    let inaccessible = match l.inaccessible_kms_key_date_time {
        Some(t) => json!(t),
        None => Value::Null,
    };
    json!({
        "Name": l.name,
        "Arn": l.arn,
        "State": l.state,
        "CreationDateTime": l.creation_date_time,
        "PermissionsMode": l.permissions_mode,
        "DeletionProtection": l.deletion_protection,
        "KmsKeyArn": l.kms_key_arn,
        "EncryptionDescription": {
            "KmsKeyArn": l.kms_key_arn,
            "EncryptionStatus": l.encryption_status,
            "InaccessibleKmsKeyDateTime": inaccessible,
        },
    })
}

pub struct QldbService {
    store: AccountRegionStore<QldbState>,
}

impl QldbService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<QldbState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<QldbState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for QldbService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for QldbService {
    fn service_name(&self) -> &str {
        "qldb"
    }

    fn signing_name(&self) -> &str {
        "qldb"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/ledgers",
                operation: "CreateLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/ledgers",
                operation: "ListLedgers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/ledgers/{name}",
                operation: "DescribeLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/ledgers/{name}",
                operation: "UpdateLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/ledgers/{name}",
                operation: "DeleteLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/ledgers/{name}/permissions-mode",
                operation: "UpdateLedgerPermissionsMode",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{resourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{resourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{resourceArn}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "QLDB request");
        let state = self.get_state(ctx);
        match operation {
            "CreateLedger" => {
                let name = require_str(&input, "Name")?.to_string();
                if state.ledgers.contains_key(&name) {
                    return Err(AwsError::conflict(
                        "ResourceAlreadyExistsException",
                        format!("Ledger {name} already exists"),
                    ));
                }
                if state.ledgers.len() >= LEDGER_QUOTA_PER_REGION {
                    return Err(AwsError::bad_request(
                        "LimitExceededException",
                        format!(
                            "Account already has {LEDGER_QUOTA_PER_REGION} ledgers in this region.",
                        ),
                    ));
                }
                let tags: HashMap<String, String> = input
                    .get("Tags")
                    .and_then(|v| v.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                let permissions_mode = require_str(&input, "PermissionsMode")?.to_string();
                if !matches!(permissions_mode.as_str(), "ALLOW_ALL" | "STANDARD") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "PermissionsMode `{permissions_mode}` must be ALLOW_ALL or STANDARD.",
                        ),
                    ));
                }
                let l = Ledger {
                    name: name.clone(),
                    arn: ledger_arn(ctx, &name),
                    state: "ACTIVE".to_string(),
                    creation_date_time: now(),
                    permissions_mode,
                    deletion_protection: input
                        .get("DeletionProtection")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    kms_key_arn: input
                        .get("KmsKey")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    tags,
                    encryption_status: default_encryption_status(),
                    inaccessible_kms_key_date_time: None,
                };
                let result = ledger_to_value(&l);
                state.ledgers.insert(name, l);
                Ok(result)
            }
            "DescribeLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let l = state.ledgers.get(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                Ok(ledger_to_value(&l))
            }
            "ListLedgers" => {
                // AWS QLDB ListLedgers caps MaxResults at 100 and uses
                // the ledger name as the NextToken cursor.
                let max_results = clamp_max_results_strict(
                    input.get("MaxResults").and_then(Value::as_i64),
                    100,
                    100,
                )?;
                let starting_token = input.get("NextToken").and_then(Value::as_str);
                let mut summaries: Vec<(String, Value)> = state
                    .ledgers
                    .iter()
                    .map(|e| {
                        let l = e.value();
                        (
                            l.name.clone(),
                            json!({
                                "Name": l.name,
                                "State": l.state,
                                "CreationDateTime": l.creation_date_time,
                            }),
                        )
                    })
                    .collect();
                summaries.sort_by(|a, b| a.0.cmp(&b.0));
                let page = paginate(summaries, max_results, starting_token, |(k, _)| k.clone())?;
                let mut body = json!({
                    "Ledgers": page.items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
                });
                if let Some(token) = page.next_token {
                    body["NextToken"] = json!(token);
                }
                Ok(body)
            }
            "UpdateLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let mut l = state.ledgers.get_mut(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                if let Some(d) = input.get("DeletionProtection").and_then(|v| v.as_bool()) {
                    l.deletion_protection = d;
                }
                if let Some(k) = input.get("KmsKey").and_then(|v| v.as_str()) {
                    l.kms_key_arn = Some(k.to_string());
                }
                Ok(ledger_to_value(&l))
            }
            "UpdateLedgerPermissionsMode" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let mode = require_str(&input, "PermissionsMode")?.to_string();
                if !matches!(mode.as_str(), "ALLOW_ALL" | "STANDARD") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!("PermissionsMode `{mode}` must be ALLOW_ALL or STANDARD."),
                    ));
                }
                let mut l = state.ledgers.get_mut(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                l.permissions_mode = mode;
                Ok(json!({
                    "Name": l.name,
                    "Arn": l.arn,
                    "PermissionsMode": l.permissions_mode,
                }))
            }
            "ExportJournalToS3" => {
                let ledger_name = require_str(&input, "name")
                    .or_else(|_| require_str(&input, "LedgerName"))?
                    .to_string();
                if !state.ledgers.contains_key(&ledger_name) {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {ledger_name} not found"),
                    ));
                }
                let inclusive_start_time = input
                    .get("InclusiveStartTime")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "InclusiveStartTime is required and must be a number",
                        )
                    })?;
                let exclusive_end_time = input
                    .get("ExclusiveEndTime")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "ExclusiveEndTime is required and must be a number",
                        )
                    })?;
                if exclusive_end_time <= inclusive_start_time {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        "ExclusiveEndTime must be strictly after InclusiveStartTime.",
                    ));
                }
                let role_arn = require_str(&input, "RoleArn")?.to_string();
                let output_format = input
                    .get("OutputFormat")
                    .and_then(Value::as_str)
                    .unwrap_or("ION_BINARY")
                    .to_string();
                if !matches!(output_format.as_str(), "ION_BINARY" | "ION_TEXT" | "JSON") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "OutputFormat `{output_format}` must be ION_BINARY, ION_TEXT, or JSON.",
                        ),
                    ));
                }
                let s3_cfg = input.get("S3ExportConfiguration").ok_or_else(|| {
                    AwsError::bad_request(
                        "ValidationException",
                        "S3ExportConfiguration is required",
                    )
                })?;
                let bucket = s3_cfg
                    .get("Bucket")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "S3ExportConfiguration.Bucket is required",
                        )
                    })?
                    .to_string();
                let prefix = s3_cfg
                    .get("Prefix")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let encryption = s3_cfg.get("EncryptionConfiguration").ok_or_else(|| {
                    AwsError::bad_request(
                        "ValidationException",
                        "S3ExportConfiguration.EncryptionConfiguration is required",
                    )
                })?;
                let object_encryption_type = encryption
                    .get("ObjectEncryptionType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "EncryptionConfiguration.ObjectEncryptionType is required",
                        )
                    })?
                    .to_string();
                if !matches!(
                    object_encryption_type.as_str(),
                    "SSE_KMS" | "SSE_S3" | "NO_ENCRYPTION"
                ) {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "ObjectEncryptionType `{object_encryption_type}` must be SSE_KMS, SSE_S3, or NO_ENCRYPTION."
                        ),
                    ));
                }
                let kms_key_arn = encryption
                    .get("KmsKeyArn")
                    .and_then(Value::as_str)
                    .map(String::from);
                if object_encryption_type == "SSE_KMS" && kms_key_arn.is_none() {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        "EncryptionConfiguration.KmsKeyArn is required when ObjectEncryptionType=SSE_KMS.",
                    ));
                }
                let export_id = uuid::Uuid::new_v4().to_string();
                let ex = JournalS3Export {
                    export_id: export_id.clone(),
                    ledger_name,
                    role_arn,
                    inclusive_start_time,
                    exclusive_end_time,
                    output_format,
                    bucket,
                    prefix,
                    object_encryption_type,
                    kms_key_arn,
                    status: "IN_PROGRESS".to_string(),
                    creation_time: now(),
                };
                state.s3_exports.insert(export_id.clone(), ex);
                Ok(json!({ "ExportId": export_id }))
            }
            "DescribeJournalS3Export" => {
                let _ledger =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                let export_id =
                    require_str(&input, "exportId").or_else(|_| require_str(&input, "ExportId"))?;
                let ex = state.s3_exports.get(export_id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Export {export_id} not found"),
                    )
                })?;
                Ok(json!({ "ExportDescription": export_to_value(&ex) }))
            }
            "ListJournalS3Exports" => {
                let max_results = clamp_max_results_strict(
                    input.get("MaxResults").and_then(Value::as_i64),
                    100,
                    100,
                )?;
                let starting_token = input.get("NextToken").and_then(Value::as_str);
                let mut exports: Vec<(String, Value)> = state
                    .s3_exports
                    .iter()
                    .map(|e| (e.value().export_id.clone(), export_to_value(e.value())))
                    .collect();
                exports.sort_by(|a, b| a.0.cmp(&b.0));
                let page = paginate(exports, max_results, starting_token, |(k, _)| k.clone())?;
                let mut body = json!({
                    "JournalS3Exports": page.items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
                });
                if let Some(token) = page.next_token {
                    body["NextToken"] = json!(token);
                }
                Ok(body)
            }
            "ListJournalS3ExportsForLedger" => {
                let ledger_name =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                if !state.ledgers.contains_key(ledger_name) {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {ledger_name} not found"),
                    ));
                }
                let max_results = clamp_max_results_strict(
                    input.get("MaxResults").and_then(Value::as_i64),
                    100,
                    100,
                )?;
                let starting_token = input.get("NextToken").and_then(Value::as_str);
                let mut exports: Vec<(String, Value)> = state
                    .s3_exports
                    .iter()
                    .filter(|e| e.value().ledger_name == ledger_name)
                    .map(|e| (e.value().export_id.clone(), export_to_value(e.value())))
                    .collect();
                exports.sort_by(|a, b| a.0.cmp(&b.0));
                let page = paginate(exports, max_results, starting_token, |(k, _)| k.clone())?;
                let mut body = json!({
                    "JournalS3Exports": page.items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
                });
                if let Some(token) = page.next_token {
                    body["NextToken"] = json!(token);
                }
                Ok(body)
            }
            "CancelJournalS3Export" => {
                let _ledger =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                let export_id =
                    require_str(&input, "exportId").or_else(|_| require_str(&input, "ExportId"))?;
                let mut ex = state.s3_exports.get_mut(export_id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Export {export_id} not found"),
                    )
                })?;
                match ex.status.as_str() {
                    "CANCELLED" => {}
                    "COMPLETED" | "FAILED" => {
                        return Err(AwsError::precondition_failed(
                            "ResourcePreconditionNotMetException",
                            format!(
                                "Export {export_id} is in terminal state `{}` and cannot be canceled.",
                                ex.status,
                            ),
                        ));
                    }
                    _ => {
                        ex.status = "CANCELLED".to_string();
                    }
                }
                Ok(json!({}))
            }
            "StreamJournalToKinesis" => {
                let ledger_name = require_str(&input, "name")
                    .or_else(|_| require_str(&input, "LedgerName"))?
                    .to_string();
                if !state.ledgers.contains_key(&ledger_name) {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {ledger_name} not found"),
                    ));
                }
                let stream_name = require_str(&input, "StreamName")?.to_string();
                let role_arn = require_str(&input, "RoleArn")?.to_string();
                let inclusive_start_time = input
                    .get("InclusiveStartTime")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "InclusiveStartTime is required and must be a number",
                        )
                    })?;
                let exclusive_end_time = input.get("ExclusiveEndTime").and_then(Value::as_f64);
                let kinesis = input.get("KinesisConfiguration").ok_or_else(|| {
                    AwsError::bad_request("ValidationException", "KinesisConfiguration is required")
                })?;
                let kinesis_stream_arn = kinesis
                    .get("StreamArn")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "KinesisConfiguration.StreamArn is required",
                        )
                    })?
                    .to_string();
                if !kinesis_stream_arn.starts_with("arn:") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "KinesisConfiguration.StreamArn `{kinesis_stream_arn}` is not a valid ARN.",
                        ),
                    ));
                }
                let aggregation_enabled = kinesis
                    .get("AggregationEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let tags: HashMap<String, String> = input
                    .get("Tags")
                    .and_then(|v| v.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                let stream_id = uuid::Uuid::new_v4().to_string();
                let stream = JournalKinesisStream {
                    stream_id: stream_id.clone(),
                    ledger_name: ledger_name.clone(),
                    stream_name,
                    role_arn,
                    kinesis_stream_arn,
                    inclusive_start_time,
                    exclusive_end_time,
                    aggregation_enabled,
                    creation_time: now(),
                    status: "ACTIVE".to_string(),
                    error_cause: None,
                    tags,
                };
                state.kinesis_streams.insert(stream_id.clone(), stream);
                Ok(json!({ "StreamId": stream_id }))
            }
            "ListJournalKinesisStreamsForLedger" => {
                let ledger_name =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                if !state.ledgers.contains_key(ledger_name) {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {ledger_name} not found"),
                    ));
                }
                let max_results = clamp_max_results_strict(
                    input.get("MaxResults").and_then(Value::as_i64),
                    100,
                    100,
                )?;
                let starting_token = input.get("NextToken").and_then(Value::as_str);
                let mut streams: Vec<(String, Value)> = state
                    .kinesis_streams
                    .iter()
                    .filter(|e| e.value().ledger_name == ledger_name)
                    .map(|e| (e.value().stream_id.clone(), stream_to_value(e.value(), ctx)))
                    .collect();
                streams.sort_by(|a, b| a.0.cmp(&b.0));
                let page = paginate(streams, max_results, starting_token, |(k, _)| k.clone())?;
                let mut body = json!({
                    "Streams": page.items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
                });
                if let Some(token) = page.next_token {
                    body["NextToken"] = json!(token);
                }
                Ok(body)
            }
            "CancelJournalKinesisStream" => {
                let _ledger =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                let stream_id =
                    require_str(&input, "streamId").or_else(|_| require_str(&input, "StreamId"))?;
                let mut s = state.kinesis_streams.get_mut(stream_id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Stream {stream_id} not found"),
                    )
                })?;
                // AWS rejects cancels against terminal states with
                // ResourcePreconditionNotMetException (HTTP 412). The
                // CANCELED case is idempotent so callers can retry.
                match s.status.as_str() {
                    "CANCELED" => {}
                    "COMPLETED" | "FAILED" => {
                        return Err(AwsError::precondition_failed(
                            "ResourcePreconditionNotMetException",
                            format!(
                                "Stream {stream_id} is in terminal state `{}` and cannot be canceled.",
                                s.status,
                            ),
                        ));
                    }
                    _ => {
                        s.status = "CANCELED".to_string();
                    }
                }
                Ok(json!({ "StreamId": stream_id }))
            }
            "DescribeJournalKinesisStream" => {
                let _ledger =
                    require_str(&input, "name").or_else(|_| require_str(&input, "LedgerName"))?;
                let stream_id =
                    require_str(&input, "streamId").or_else(|_| require_str(&input, "StreamId"))?;
                let s = state.kinesis_streams.get(stream_id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Stream {stream_id} not found"),
                    )
                })?;
                Ok(json!({ "Stream": stream_to_value(&s, ctx) }))
            }
            "DeleteLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let l = state.ledgers.get(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                if l.deletion_protection {
                    return Err(AwsError::precondition_failed(
                        "ResourcePreconditionNotMetException",
                        "Disable DeletionProtection before deleting the ledger",
                    ));
                }
                drop(l);
                state.ledgers.remove(name);
                Ok(json!({}))
            }
            "TagResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let new_tags: HashMap<String, String> = input
                    .get("Tags")
                    .or_else(|| input.get("tags"))
                    .and_then(|v| v.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                apply_resource_tags(&state, arn, new_tags)?;
                Ok(json!({}))
            }
            "UntagResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let keys: Vec<String> = input
                    .get("TagKeys")
                    .or_else(|| input.get("tagKeys"))
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                remove_resource_tags(&state, arn, &keys)?;
                Ok(json!({}))
            }
            "ListTagsForResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tags = read_resource_tags(&state, arn)?;
                Ok(json!({ "Tags": tags }))
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = QldbSnapshot {
            ledgers: vec![],
            kinesis_streams: vec![],
            s3_exports: vec![],
            resource_tags: Default::default(),
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.ledgers.extend(s.ledgers);
            all.kinesis_streams.extend(s.kinesis_streams);
            all.s3_exports.extend(s.s3_exports);
            all.resource_tags.extend(s.resource_tags);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: QldbSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("qldb", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    #[test]
    fn deletion_protection_blocks_delete() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "audit", "PermissionsMode": "STANDARD" }),
            &ctx,
        ))
        .unwrap();
        let err =
            block_on(svc.handle("DeleteLedger", json!({ "name": "audit" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ResourcePreconditionNotMetException");
        block_on(svc.handle(
            "UpdateLedger",
            json!({ "name": "audit", "DeletionProtection": false }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle("DeleteLedger", json!({ "name": "audit" }), &ctx)).unwrap();
    }

    #[test]
    fn create_ledger_enforces_per_region_quota() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        for i in 0..LEDGER_QUOTA_PER_REGION {
            block_on(svc.handle(
                "CreateLedger",
                json!({
                    "Name": format!("led-{i}"),
                    "PermissionsMode": "STANDARD",
                    "DeletionProtection": false,
                }),
                &ctx,
            ))
            .unwrap();
        }
        let err = block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "led-extra",
                "PermissionsMode": "STANDARD",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "LimitExceededException");
    }

    #[test]
    fn deletion_protection_returns_412() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "p412",
                "PermissionsMode": "STANDARD",
                "DeletionProtection": true,
            }),
            &ctx,
        ))
        .unwrap();
        let err =
            block_on(svc.handle("DeleteLedger", json!({ "name": "p412" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ResourcePreconditionNotMetException");
        assert_eq!(err.status.as_u16(), 412);
    }

    #[test]
    fn update_ledger_accepts_kms_key_and_surfaces_encryption_description() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "kms-led", "PermissionsMode": "STANDARD", "DeletionProtection": false }),
            &ctx,
        ))
        .unwrap();

        let kms_key = "arn:aws:kms:us-east-1:123456789012:key/abcdef01-2345-6789-abcd-ef0123456789";
        let resp = block_on(svc.handle(
            "UpdateLedger",
            json!({ "name": "kms-led", "KmsKey": kms_key }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["KmsKeyArn"], kms_key);
        let enc = &resp["EncryptionDescription"];
        assert_eq!(enc["KmsKeyArn"], kms_key);
        assert_eq!(enc["EncryptionStatus"], "ENABLED");
        assert!(
            enc.get("InaccessibleKmsKeyDateTime")
                .map(|v| v.is_null())
                .unwrap_or(false),
            "expected InaccessibleKmsKeyDateTime to be present as null, got {enc:?}",
        );
    }

    #[test]
    fn update_ledger_permissions_mode_persists() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "audit-mode",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();

        let resp = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "STANDARD" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["PermissionsMode"], "STANDARD");

        let described =
            block_on(svc.handle("DescribeLedger", json!({ "name": "audit-mode" }), &ctx)).unwrap();
        assert_eq!(described["PermissionsMode"], "STANDARD");

        // Round trip via UpdateLedgerPermissionsMode for the other variant.
        let resp = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "ALLOW_ALL" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["PermissionsMode"], "ALLOW_ALL");

        // Bad value still rejected.
        let err = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "ROOT" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_ledger_rejects_unknown_permissions_mode() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        let err = block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "x", "PermissionsMode": "WIDE_OPEN" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn list_ledgers_paginates_with_max_results_and_next_token() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        for name in ["alpha", "bravo", "charlie", "delta", "echo"] {
            block_on(svc.handle(
                "CreateLedger",
                json!({
                    "Name": name,
                    "PermissionsMode": "ALLOW_ALL",
                    "DeletionProtection": false,
                }),
                &ctx,
            ))
            .unwrap();
        }
        let first = block_on(svc.handle("ListLedgers", json!({ "MaxResults": 2 }), &ctx)).unwrap();
        let names: Vec<String> = first["Ledgers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "bravo"]);
        let token = first["NextToken"].as_str().unwrap().to_string();
        let second = block_on(svc.handle(
            "ListLedgers",
            json!({ "MaxResults": 2, "NextToken": token }),
            &ctx,
        ))
        .unwrap();
        let names: Vec<String> = second["Ledgers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["charlie", "delta"]);
    }

    #[test]
    fn list_ledgers_rejects_max_results_out_of_range() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        for bad in [0i64, -1, 101, 1000] {
            let err = block_on(svc.handle("ListLedgers", json!({ "MaxResults": bad }), &ctx))
                .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
    }

    #[test]
    fn create_ledger_accepts_documented_permissions_modes() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        for mode in ["ALLOW_ALL", "STANDARD"] {
            block_on(svc.handle(
                "CreateLedger",
                json!({
                    "Name": format!("ledger-{mode}"),
                    "PermissionsMode": mode,
                    "DeletionProtection": false,
                }),
                &ctx,
            ))
            .unwrap();
        }
    }

    #[test]
    fn export_journal_to_s3_persists_and_describes_export() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "exp-target",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let resp = block_on(svc.handle(
            "ExportJournalToS3",
            json!({
                "name": "exp-target",
                "InclusiveStartTime": 1700000000.0,
                "ExclusiveEndTime": 1700003600.0,
                "S3ExportConfiguration": {
                    "Bucket": "audit-out",
                    "Prefix": "exp/",
                    "EncryptionConfiguration": {
                        "ObjectEncryptionType": "SSE_KMS",
                        "KmsKeyArn": "arn:aws:kms:us-east-1:000000000000:key/abc",
                    },
                },
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-export",
                "OutputFormat": "JSON",
            }),
            &ctx,
        ))
        .unwrap();
        let export_id = resp["ExportId"].as_str().unwrap().to_string();
        let desc = block_on(svc.handle(
            "DescribeJournalS3Export",
            json!({ "name": "exp-target", "exportId": export_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["ExportDescription"]["Status"], "IN_PROGRESS");
        assert_eq!(desc["ExportDescription"]["OutputFormat"], "JSON");
        assert_eq!(
            desc["ExportDescription"]["S3ExportConfiguration"]["EncryptionConfiguration"]["ObjectEncryptionType"],
            "SSE_KMS"
        );
    }

    #[test]
    fn export_journal_to_s3_rejects_sse_kms_without_key_arn() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "exp-bad",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "ExportJournalToS3",
            json!({
                "name": "exp-bad",
                "InclusiveStartTime": 1700000000.0,
                "ExclusiveEndTime": 1700003600.0,
                "S3ExportConfiguration": {
                    "Bucket": "audit-out",
                    "EncryptionConfiguration": { "ObjectEncryptionType": "SSE_KMS" },
                },
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-export",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn cancel_journal_s3_export_marks_cancelled_idempotently() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "exp-cancel",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let resp = block_on(svc.handle(
            "ExportJournalToS3",
            json!({
                "name": "exp-cancel",
                "InclusiveStartTime": 1700000000.0,
                "ExclusiveEndTime": 1700003600.0,
                "S3ExportConfiguration": {
                    "Bucket": "audit-out",
                    "EncryptionConfiguration": { "ObjectEncryptionType": "NO_ENCRYPTION" },
                },
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-export",
            }),
            &ctx,
        ))
        .unwrap();
        let export_id = resp["ExportId"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CancelJournalS3Export",
            json!({ "name": "exp-cancel", "exportId": export_id }),
            &ctx,
        ))
        .unwrap();
        // Idempotent
        block_on(svc.handle(
            "CancelJournalS3Export",
            json!({ "name": "exp-cancel", "exportId": export_id }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle(
            "DescribeJournalS3Export",
            json!({ "name": "exp-cancel", "exportId": export_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["ExportDescription"]["Status"], "CANCELLED");
    }

    #[test]
    fn list_journal_kinesis_streams_for_ledger_filters_by_ledger() {
        let svc = QldbService::new();
        let ctx = ctx();
        for name in ["one", "two"] {
            block_on(svc.handle(
                "CreateLedger",
                json!({
                    "Name": name,
                    "PermissionsMode": "ALLOW_ALL",
                    "DeletionProtection": false,
                }),
                &ctx,
            ))
            .unwrap();
        }
        for ledger in ["one", "one", "two"] {
            block_on(svc.handle(
                "StreamJournalToKinesis",
                json!({
                    "name": ledger,
                    "StreamName": format!("s-{ledger}"),
                    "RoleArn": "arn:aws:iam::000000000000:role/qldb",
                    "InclusiveStartTime": 1700000000.0,
                    "KinesisConfiguration": {
                        "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/k",
                    },
                }),
                &ctx,
            ))
            .unwrap();
        }
        let resp = block_on(svc.handle(
            "ListJournalKinesisStreamsForLedger",
            json!({ "name": "one" }),
            &ctx,
        ))
        .unwrap();
        let streams = resp["Streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2);
        assert!(streams.iter().all(|s| s["LedgerName"] == "one"));
    }

    #[test]
    fn cancel_journal_kinesis_stream_marks_canceled_idempotently() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "cancel-target",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let created = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "cancel-target",
                "StreamName": "cancel-stream",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {
                    "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/k",
                },
            }),
            &ctx,
        ))
        .unwrap();
        let stream_id = created["StreamId"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CancelJournalKinesisStream",
            json!({ "name": "cancel-target", "streamId": stream_id }),
            &ctx,
        ))
        .unwrap();
        // Idempotent: a second cancel succeeds.
        block_on(svc.handle(
            "CancelJournalKinesisStream",
            json!({ "name": "cancel-target", "streamId": stream_id }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle(
            "DescribeJournalKinesisStream",
            json!({ "name": "cancel-target", "streamId": stream_id }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["Stream"]["Status"], "CANCELED");
    }

    #[test]
    fn cancel_journal_kinesis_stream_rejects_terminal_state() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "completed-ledger",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let created = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "completed-ledger",
                "StreamName": "done",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {
                    "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/k",
                },
            }),
            &ctx,
        ))
        .unwrap();
        let stream_id = created["StreamId"].as_str().unwrap().to_string();
        // Force COMPLETED on the persisted record so the cancel hits a
        // terminal state.
        {
            let st = svc.store.get("000000000000", "us-east-1");
            let mut entry = st.kinesis_streams.get_mut(&stream_id).unwrap();
            entry.status = "COMPLETED".to_string();
        }
        let err = block_on(svc.handle(
            "CancelJournalKinesisStream",
            json!({ "name": "completed-ledger", "streamId": stream_id }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourcePreconditionNotMetException");
        assert_eq!(err.status.as_u16(), 412);
    }

    #[test]
    fn stream_journal_to_kinesis_persists_and_describes_stream() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "audit",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let resp = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "audit",
                "StreamName": "audit-stream",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-stream",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {
                    "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/audit-out",
                    "AggregationEnabled": true,
                },
            }),
            &ctx,
        ))
        .unwrap();
        let stream_id = resp["StreamId"].as_str().unwrap().to_string();
        let desc = block_on(svc.handle(
            "DescribeJournalKinesisStream",
            json!({ "name": "audit", "streamId": stream_id }),
            &ctx,
        ))
        .unwrap();
        let s = &desc["Stream"];
        assert_eq!(s["LedgerName"], "audit");
        assert_eq!(s["StreamName"], "audit-stream");
        assert_eq!(s["Status"], "ACTIVE");
        assert_eq!(s["KinesisConfiguration"]["AggregationEnabled"], true);
    }

    #[test]
    fn stream_journal_to_kinesis_rejects_unknown_ledger() {
        let svc = QldbService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "ghost",
                "StreamName": "audit-stream",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-stream",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {
                    "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/x",
                },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn stream_journal_to_kinesis_requires_kinesis_stream_arn() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "audit2",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "audit2",
                "StreamName": "audit-stream",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb-stream",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {},
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn tag_resource_returns_not_found_for_unknown_ledger() {
        let svc = QldbService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "TagResource",
            json!({
                "resourceArn": "arn:aws:qldb:us-east-1:000000000000:ledger/missing",
                "Tags": { "team": "qldb" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn untag_resource_returns_not_found_for_unknown_ledger() {
        let svc = QldbService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "UntagResource",
            json!({
                "resourceArn": "arn:aws:qldb:us-east-1:000000000000:ledger/missing",
                "TagKeys": ["team"],
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn list_tags_returns_not_found_for_unknown_ledger() {
        let svc = QldbService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "resourceArn": "arn:aws:qldb:us-east-1:000000000000:ledger/missing" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn tag_resource_round_trips_stream_tags() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "tagged-stream-ledger",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let created = block_on(svc.handle(
            "StreamJournalToKinesis",
            json!({
                "name": "tagged-stream-ledger",
                "StreamName": "stream-x",
                "RoleArn": "arn:aws:iam::000000000000:role/qldb",
                "InclusiveStartTime": 1700000000.0,
                "KinesisConfiguration": {
                    "StreamArn": "arn:aws:kinesis:us-east-1:000000000000:stream/x",
                },
            }),
            &ctx,
        ))
        .unwrap();
        let stream_id = created["StreamId"].as_str().unwrap().to_string();
        let stream_arn =
            format!("arn:aws:qldb:us-east-1:000000000000:stream/tagged-stream-ledger/{stream_id}",);
        block_on(svc.handle(
            "TagResource",
            json!({ "resourceArn": stream_arn, "Tags": { "team": "qldb" } }),
            &ctx,
        ))
        .unwrap();
        let listed = block_on(svc.handle(
            "ListTagsForResource",
            json!({ "resourceArn": stream_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(listed["Tags"]["team"], "qldb");
    }

    #[test]
    fn tag_resource_returns_not_found_for_unknown_stream() {
        let svc = QldbService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "TagResource",
            json!({
                "resourceArn": "arn:aws:qldb:us-east-1:000000000000:stream/missing-ledger/missing-stream",
                "Tags": {},
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn tag_resource_round_trips_tags() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "tagged",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();
        let arn = "arn:aws:qldb:us-east-1:000000000000:ledger/tagged";
        block_on(svc.handle(
            "TagResource",
            json!({ "resourceArn": arn, "Tags": { "team": "qldb", "env": "test" } }),
            &ctx,
        ))
        .unwrap();
        let resp = block_on(svc.handle("ListTagsForResource", json!({ "resourceArn": arn }), &ctx))
            .unwrap();
        assert_eq!(resp["Tags"]["team"], "qldb");
        assert_eq!(resp["Tags"]["env"], "test");
        block_on(svc.handle(
            "UntagResource",
            json!({ "resourceArn": arn, "TagKeys": ["team"] }),
            &ctx,
        ))
        .unwrap();
        let resp = block_on(svc.handle("ListTagsForResource", json!({ "resourceArn": arn }), &ctx))
            .unwrap();
        assert!(resp["Tags"]["team"].is_null());
        assert_eq!(resp["Tags"]["env"], "test");
    }
}
