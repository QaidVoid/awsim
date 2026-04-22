use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;

/// Global OpenSearch state (not per-account — OpenSearch domains are accessed directly).
#[derive(Default)]
pub struct OpenSearchState {
    /// Index name → Index data
    pub indices: DashMap<String, OpenSearchIndex>,
}

/// An OpenSearch/Elasticsearch index.
pub struct OpenSearchIndex {
    pub name: String,
    pub mappings: Value,
    pub settings: Value,
    /// Document ID → document source
    pub documents: HashMap<String, Value>,
    pub created_at: String,
}
