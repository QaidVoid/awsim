use std::collections::{BTreeMap, HashMap};

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

/// A DynamoDB Table.
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
    /// Composite key (pk\0sk or pk alone) → item.
    pub items: BTreeMap<String, DynamoItem>,
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
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

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

    /// Build the composite storage key from an item or key map.
    ///
    /// Unused after the SQLite refactor — kept around because tests and
    /// future stages (transactional ranges, GSI projection) still want the
    /// `pk\0sk` form for in-memory comparisons.
    #[allow(dead_code)]
    pub fn composite_key(&self, item: &DynamoItem) -> Option<String> {
        let hk = self.hash_key()?;
        let pk_val = extract_scalar_str(item.get(hk)?)?;
        if let Some(rk) = self.range_key() {
            let sk_val = extract_scalar_str(item.get(rk)?)?;
            Some(format!("{pk_val}\0{sk_val}"))
        } else {
            Some(pk_val.to_string())
        }
    }

    /// Return the partition key value as string from a composite key.
    #[allow(dead_code)]
    pub fn pk_from_composite<'a>(&self, composite: &'a str) -> &'a str {
        match composite.find('\0') {
            Some(idx) => &composite[..idx],
            None => composite,
        }
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

#[derive(Debug, Default)]
pub struct DynamoState {
    pub tables: DashMap<String, Table>,
    pub backups: DashMap<String, BackupRecord>,
    pub exports: DashMap<String, ExportRecord>,
    pub imports: DashMap<String, ImportRecord>,
    pub kinesis_destinations: DashMap<String, Vec<KinesisStreamingDestination>>,
    pub pitr_enabled: DashMap<String, bool>,
    pub resource_policies: DashMap<String, String>,
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("name", &self.name)
            .field("status", &self.status)
            .field("item_count", &self.items.len())
            .finish()
    }
}
