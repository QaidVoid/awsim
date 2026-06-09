use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use awsim_core::AwsError;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::throttle::{BucketKind, ThrottleRegistry};

/// Data captured for a single item change in a DynamoDB Stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRecordData {
    /// Key attribute(s) of the modified item.
    pub keys: HashMap<String, Value>,
    /// Image of the item after the modification (INSERT / MODIFY).
    pub new_image: Option<HashMap<String, Value>>,
    /// Image of the item before the modification (MODIFY / REMOVE).
    pub old_image: Option<HashMap<String, Value>>,
    /// Monotonically increasing sequence number within the stream.
    pub sequence_number: String,
    /// Approximate size of the record in bytes.
    pub size_bytes: u64,
    /// The owning table's StreamViewType: "KEYS_ONLY", "NEW_IMAGE",
    /// "OLD_IMAGE", or "NEW_AND_OLD_IMAGES". Determines which of
    /// `new_image` / `old_image` are populated.
    pub stream_view_type: String,
}

/// A single stream record representing one item-level DynamoDB change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRecord {
    /// Globally unique identifier for this stream event.
    pub event_id: String,
    /// "INSERT", "MODIFY", or "REMOVE".
    pub event_name: String,
    /// The change data payload.
    pub dynamodb: StreamRecordData,
    /// ARN of the stream this record belongs to.
    pub event_source_arn: String,
}

/// A DynamoDB attribute value (typed).
/// Keys are the type discriminator: "S", "N", "B", "BOOL", "NULL", "L", "M", "SS", "NS", "BS".
pub type DynamoItem = HashMap<String, Value>;

/// Key schema element: hash key or range key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySchemaElement {
    pub attribute_name: String,
    /// "HASH" or "RANGE"
    pub key_type: String,
}

/// Attribute definition: type of an attribute used in key schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDefinition {
    pub attribute_name: String,
    /// "S", "N", or "B"
    pub attribute_type: String,
}

/// Projection type for secondary indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projection {
    pub projection_type: String, // ALL | KEYS_ONLY | INCLUDE
    pub non_key_attributes: Vec<String>,
}

/// Global Secondary Index definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSecondaryIndex {
    pub index_name: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub projection: Projection,
    pub status: String,
}

/// Local Secondary Index definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSecondaryIndex {
    pub index_name: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub projection: Projection,
}

/// TTL specification for a table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TtlSpecification {
    /// Whether TTL is enabled.
    pub enabled: bool,
    /// The attribute name used for TTL.
    pub attribute_name: String,
}

/// Server-side encryption metadata. AWS-owned-key encryption is the
/// default and AWS doesn't surface an `SSEDescription` in DescribeTable
/// for it (so `enabled = false` here means "default-managed", which
/// is the absence of customer-managed encryption rather than "no
/// encryption at all"). When `enabled = true` the table reports
/// `SSEDescription` to the client; we don't actually encrypt anything
/// in awsim — it's metadata only so SDK code that round-trips it
/// works.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SseSpecification {
    pub enabled: bool,
    /// "AES256" (AWS-owned key) or "KMS" (customer-managed). Empty
    /// when `enabled = false`.
    #[serde(default)]
    pub sse_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kms_master_key_arn: Option<String>,
}

/// A DynamoDB Table — schema + stream config only.
///
/// Items live in SQLite (see `SqliteStore`); this struct holds the
/// metadata that operation handlers need to answer DescribeTable,
/// resolve key schemas for indexing, and run stream emission.
#[derive(Clone, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub arn: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub attribute_definitions: Vec<AttributeDefinition>,
    pub billing_mode: String,
    pub status: String,
    /// Unix epoch seconds — matches awsJson1.1 timestamp wire format.
    pub created_at: f64,
    pub gsi: Vec<GlobalSecondaryIndex>,
    pub lsi: Vec<LocalSecondaryIndex>,
    /// Whether DynamoDB Streams is enabled for this table.
    #[serde(default)]
    pub stream_enabled: bool,
    /// Stream ARN when streaming is enabled.
    #[serde(default)]
    pub stream_arn: Option<String>,
    /// View type for the stream (e.g. "NEW_AND_OLD_IMAGES").
    #[serde(default)]
    pub stream_view_type: Option<String>,
    /// Bounded ring buffer of recent stream records (last 1 000).
    #[serde(default)]
    pub stream_records: VecDeque<StreamRecord>,
    /// Monotonically increasing counter used to generate sequence numbers.
    #[serde(default)]
    pub stream_sequence: u64,
    /// Time-to-Live specification.
    #[serde(default)]
    pub ttl: TtlSpecification,
    /// Resource tags (key → value).
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// When true, `DeleteTable` rejects the request — callers must
    /// flip this off via `UpdateTable` first. Mirrors the AWS
    /// `DeletionProtectionEnabled` table attribute.
    #[serde(default)]
    pub deletion_protection_enabled: bool,
    /// Server-side encryption settings. Metadata only — awsim doesn't
    /// actually encrypt items, but echoing the spec back keeps SDK
    /// code that reads `SSEDescription` happy.
    #[serde(default)]
    pub sse: SseSpecification,
    /// Provisioned read capacity units. Only meaningful when
    /// `billing_mode == "PROVISIONED"`; PAY_PER_REQUEST always
    /// reports 0. Awsim doesn't actually rate-limit — the value
    /// round-trips through DescribeTable for SDK code that reads it.
    #[serde(default)]
    pub read_capacity_units: u64,
    /// Provisioned write capacity units. Same caveat as
    /// `read_capacity_units`.
    #[serde(default)]
    pub write_capacity_units: u64,
}

/// Serializable snapshot of `DynamoState`.
#[derive(Serialize, Deserialize)]
pub struct DynamoStateSnapshot {
    pub tables: Vec<Table>,
}

impl Table {
    /// Return the hash (partition) key attribute name.
    pub fn hash_key(&self) -> Option<&str> {
        self.key_schema
            .iter()
            .find(|k| k.key_type == "HASH")
            .map(|k| k.attribute_name.as_str())
    }

    /// Return the range (sort) key attribute name, if any.
    pub fn range_key(&self) -> Option<&str> {
        self.key_schema
            .iter()
            .find(|k| k.key_type == "RANGE")
            .map(|k| k.attribute_name.as_str())
    }
}

/// Extract a scalar string representation from a DynamoDB typed value.
/// Works for S, N, B types (used for key comparisons).
pub fn extract_scalar_str(val: &Value) -> Option<&str> {
    if let Some(s) = val.get("S").and_then(|v| v.as_str()) {
        return Some(s);
    }
    if let Some(n) = val.get("N").and_then(|v| v.as_str()) {
        return Some(n);
    }
    if let Some(b) = val.get("B").and_then(|v| v.as_str()) {
        return Some(b);
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub backup_arn: String,
    pub backup_name: String,
    pub table_name: String,
    pub table_arn: String,
    pub backup_status: String,
    pub backup_type: String,
    pub backup_creation_date_time: f64,
    pub backup_size_bytes: u64,
    /// Captured table schema at backup time. Used by
    /// `RestoreTableFromBackup` to rebuild the table.
    #[serde(default)]
    pub schema_snapshot: Option<Table>,
    /// Captured items as raw `(pk, sk, attrs_json)` triples — same
    /// shape SqliteStore stores them. Restored verbatim. Empty when
    /// the backup pre-dates this field on disk.
    #[serde(default)]
    pub items: Vec<BackupItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupItem {
    pub pk: String,
    pub sk: String,
    /// JSON-encoded item attributes — DynamoDB wire shape.
    pub attrs: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRecord {
    pub export_arn: String,
    pub table_arn: String,
    pub export_status: String,
    pub export_format: String,
    pub s3_bucket: String,
    pub s3_prefix: Option<String>,
    pub start_time: f64,
    pub end_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRecord {
    pub import_arn: String,
    pub table_arn: String,
    pub table_name: String,
    pub import_status: String,
    pub input_format: String,
    pub s3_bucket: String,
    pub start_time: f64,
    pub end_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KinesisStreamingDestination {
    pub stream_arn: String,
    pub destination_status: String,
    pub approximate_creation_date_time_precision: String,
}

/// One regional replica of a DynamoDB Global Table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalTableReplica {
    pub region_name: String,
    /// "CREATING" | "UPDATING" | "DELETING" | "ACTIVE"
    pub replica_status: String,
}

/// A DynamoDB Global Table — a logical group of regional replicas that share
/// a single name. We don't actually replicate data; the Global Table object
/// is just metadata that satisfies tooling (Terraform, CDK) which consults
/// existence + replica list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalTable {
    pub global_table_name: String,
    pub global_table_arn: String,
    pub creation_date: f64,
    /// "ACTIVE" | "CREATING" | "UPDATING" | "DELETING"
    pub global_table_status: String,
    pub replication_group: Vec<GlobalTableReplica>,
}

/// A cached transactional response, keyed by `ClientRequestToken`, used to make
/// `TransactWriteItems` and `ExecuteTransaction` idempotent within AWS's
/// 10-minute window.
#[derive(Debug, Clone)]
pub struct IdempotencyEntry {
    /// Hash of the request payload (minus the token) seen with this token. A
    /// later request with the same token but a different fingerprint is a
    /// parameter mismatch.
    pub fingerprint: u64,
    /// The successful response to replay for a matching retry.
    pub response: Value,
    /// Unix epoch seconds when the entry was stored, for window expiry.
    pub stored_at: f64,
}

#[derive(Default)]
pub struct DynamoState {
    pub tables: DashMap<String, Table>,
    pub backups: DashMap<String, BackupRecord>,
    pub exports: DashMap<String, ExportRecord>,
    pub imports: DashMap<String, ImportRecord>,
    pub kinesis_destinations: DashMap<String, Vec<KinesisStreamingDestination>>,
    pub pitr_enabled: DashMap<String, bool>,
    pub resource_policies: DashMap<String, String>,
    /// Global tables keyed by GlobalTableName. The implementation models
    /// the metadata only — there's no cross-region data replication.
    pub global_tables: DashMap<String, GlobalTable>,
    /// Idempotency cache for transactional writes, keyed by
    /// `{account}:{region}:{ClientRequestToken}`. Ephemeral (not snapshotted);
    /// entries expire after a 10-minute window.
    pub idempotency: DashMap<String, IdempotencyEntry>,
    /// Per-table token buckets driving `BillingMode == PROVISIONED`
    /// throttling. Looked up after each item / query / batch op so
    /// over-quota requests get a real
    /// `ProvisionedThroughputExceededException` instead of the
    /// silently-unmetered behaviour earlier versions had. Bypassed
    /// for `PAY_PER_REQUEST` tables (the supported "no throttling"
    /// path).
    pub throttle: Arc<ThrottleRegistry>,
}

impl std::fmt::Debug for DynamoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Skip `throttle` (interior mutexes don't impl Debug
        // through the registry's lock guards).
        f.debug_struct("DynamoState")
            .field("tables", &self.tables)
            .field("backups", &self.backups)
            .field("exports", &self.exports)
            .field("imports", &self.imports)
            .field("kinesis_destinations", &self.kinesis_destinations)
            .field("pitr_enabled", &self.pitr_enabled)
            .field("resource_policies", &self.resource_policies)
            .field("global_tables", &self.global_tables)
            .field("idempotency", &self.idempotency)
            .finish()
    }
}

impl DynamoState {
    /// Charge `units` against the table's read or write bucket.
    /// `Ok(())` when the request is within budget *or* the table
    /// is on `PAY_PER_REQUEST` (no enforcement). Otherwise returns
    /// `ProvisionedThroughputExceededException`.
    ///
    /// Looking up the table inside the helper keeps the call
    /// site at every operation a single line and centralises the
    /// PAY_PER_REQUEST short-circuit so callers can't forget it.
    pub fn enforce_throughput(
        &self,
        table_name: &str,
        kind: BucketKind,
        units: f64,
    ) -> Result<(), AwsError> {
        // Floor at the AWS minimum charge of one unit per call.
        // Real DynamoDB never bills less than 1 RCU / 1 WCU even
        // for empty results, and rounding here prevents a series
        // of empty-response Query / Scan calls from getting free
        // capacity.
        let charge = units.max(1.0);
        let Some(table) = self.tables.get(table_name) else {
            // Operation handlers already return the canonical
            // ResourceNotFoundException when the table is
            // missing; treating that as "no enforcement" here
            // keeps behaviour identical for existing tests.
            return Ok(());
        };
        if !table.billing_mode.eq_ignore_ascii_case("PROVISIONED") {
            return Ok(());
        }
        let read_rate = table.read_capacity_units as f64;
        let write_rate = table.write_capacity_units as f64;
        drop(table);
        self.throttle
            .enforce(table_name, kind, charge, read_rate, write_rate)
    }
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("name", &self.name)
            .field("status", &self.status)
            .finish()
    }
}
