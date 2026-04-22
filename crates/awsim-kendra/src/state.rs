use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;

/// Per-account/region Kendra state.
#[derive(Default)]
pub struct KendraState {
    pub indexes: DashMap<String, KendraIndex>,
}

/// A Kendra index that stores documents and supports search queries.
pub struct KendraIndex {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub description: String,
    pub role_arn: String,
    pub edition: String, // DEVELOPER_EDITION, ENTERPRISE_EDITION
    pub status: String,  // ACTIVE
    pub created_at: String,
    pub updated_at: String,
    pub documents: Vec<IndexedDocument>,
    pub data_sources: HashMap<String, DataSource>,
    pub faqs: HashMap<String, Faq>,
}

/// A document indexed in Kendra.
pub struct IndexedDocument {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub content_type: String, // PLAIN_TEXT, HTML
    pub attributes: HashMap<String, DocumentAttribute>,
    pub created_at: String,
}

/// A document attribute value.
pub struct DocumentAttribute {
    pub key: String,
    pub value: DocumentAttributeValue,
}

/// Typed attribute value.
pub enum DocumentAttributeValue {
    StringValue(String),
    StringListValue(Vec<String>),
    LongValue(i64),
    DateValue(String),
}

/// A data source connector.
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub ds_type: String, // S3, DATABASE, CUSTOM, etc.
    pub configuration: Value,
    pub role_arn: String,
    pub status: String,
    pub created_at: String,
}

/// A FAQ.
pub struct Faq {
    pub id: String,
    pub name: String,
    pub description: String,
    pub s3_path: Option<String>,
    pub role_arn: String,
    pub status: String,
    pub created_at: String,
}
