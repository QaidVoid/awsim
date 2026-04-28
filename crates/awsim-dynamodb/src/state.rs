use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Always "NEW_AND_OLD_IMAGES" for AWSim.
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

/// A DynamoDB Table — schema + stream config only.
///
/// Items live in SQLite (see `SqliteStore`); this struct holds the
/// metadata that operation handlers need to answer DescribeTable,
/// resolve key schemas for indexing, and run stream emission.
#[derive(Serialize, Deserialize)]
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
    pub stream_records: Vec<StreamRecord>,
    /// Monotonically increasing counter used to generate sequence numbers.
    #[serde(default)]
    pub stream_sequence: u64,
    /// Time-to-Live specification.
    #[serde(default)]
    pub ttl: TtlSpecification,
    /// Resource tags (key → value).
    #[serde(default)]
    pub tags: HashMap<String, String>,
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

#[derive(Debug, Default)]
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
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("name", &self.name)
            .field("status", &self.status)
            .finish()
    }
}
