use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A Glue/Athena workgroup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkGroup {
    pub name: String,
    pub description: Option<String>,
    pub state: String, // ENABLED | DISABLED
    pub output_location: Option<String>,
    pub created_at: String,
}

/// An Athena query execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryExecution {
    pub id: String,
    pub query: String,
    pub database: Option<String>,
    pub catalog: Option<String>,
    pub workgroup: String,
    pub output_location: Option<String>,
    /// Always "SUCCEEDED" in the stub.
    pub status: String,
    pub submitted_at: String,
    pub completed_at: String,
}

/// A named (saved) query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedQuery {
    pub id: String,
    pub name: String,
    pub database: String,
    pub query_string: String,
    pub workgroup: String,
    pub description: Option<String>,
}

/// An Athena data catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCatalog {
    pub name: String,
    pub catalog_type: String, // LAMBDA | GLUE | HIVE
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// An Athena prepared statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedStatement {
    pub statement_name: String,
    pub workgroup: String,
    pub query_statement: String,
    pub description: Option<String>,
    pub last_modified_time: String,
}

/// Per-account/region Athena state.
#[derive(Debug, Default)]
pub struct AthenaState {
    /// WorkGroup name → WorkGroup
    pub workgroups: DashMap<String, WorkGroup>,
    /// QueryExecutionId → QueryExecution
    pub query_executions: DashMap<String, QueryExecution>,
    /// NamedQueryId → NamedQuery
    pub named_queries: DashMap<String, NamedQuery>,
    /// CatalogName → DataCatalog
    pub data_catalogs: DashMap<String, DataCatalog>,
    /// "{workgroup}/{statement_name}" → PreparedStatement
    pub prepared_statements: DashMap<String, PreparedStatement>,
    /// Resource ARN → tag key/value map
    pub resource_tags: DashMap<String, HashMap<String, String>>,
}

impl AthenaState {
    /// Called lazily on first use; ensures the built-in `primary` workgroup exists.
    pub fn ensure_primary_workgroup(&self, now: &str) {
        self.workgroups
            .entry("primary".to_string())
            .or_insert_with(|| WorkGroup {
                name: "primary".to_string(),
                description: Some("Primary workgroup".to_string()),
                state: "ENABLED".to_string(),
                output_location: None,
                created_at: now.to_string(),
            });
    }

    /// Called lazily on first use; ensures the built-in AwsDataCatalog exists.
    pub fn ensure_default_catalog(&self) {
        self.data_catalogs
            .entry("AwsDataCatalog".to_string())
            .or_insert_with(|| DataCatalog {
                name: "AwsDataCatalog".to_string(),
                catalog_type: "GLUE".to_string(),
                description: Some("The AWS Glue Data Catalog".to_string()),
                parameters: serde_json::json!({}),
            });
    }
}
