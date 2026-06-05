use awsim_core::idempotency::IdempotencyCache;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Convert a stored timestamp string into the JSON number (epoch seconds)
/// that the awsJson protocol requires for `smithy.api#timestamp` members.
/// Stored values are epoch seconds; unparseable input falls back to 0.
pub(crate) fn ts_num(s: &str) -> Value {
    serde_json::json!(s.parse::<i64>().unwrap_or(0))
}

/// A Glue/Athena workgroup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkGroup {
    pub name: String,
    pub description: Option<String>,
    pub state: String, // ENABLED | DISABLED
    pub output_location: Option<String>,
    pub created_at: String,
    /// User-selected Athena engine version (e.g. `AUTO`,
    /// `Athena engine version 2`, `Athena engine version 3`). Defaults
    /// to `AUTO` if the caller omits it.
    #[serde(default = "default_selected_engine")]
    pub selected_engine_version: String,
    /// Engine actually applied at runtime; for `AUTO` this is the
    /// latest production engine, otherwise it mirrors the selection.
    #[serde(default = "default_effective_engine")]
    pub effective_engine_version: String,
}

fn default_selected_engine() -> String {
    "AUTO".to_string()
}

fn default_effective_engine() -> String {
    "Athena engine version 3".to_string()
}

/// Resolve a caller-provided `SelectedEngineVersion` to the
/// `(selected, effective)` pair AWS would return. `AUTO` (and unset)
/// resolves to engine 3.
pub fn resolve_engine_version(selected: Option<&str>) -> (String, String) {
    let sel = selected.unwrap_or("AUTO");
    let eff = match sel {
        "AUTO" => "Athena engine version 3",
        other => other,
    };
    (sel.to_string(), eff.to_string())
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
    /// `StartQueryExecution.ClientRequestToken` cache. A replay with
    /// the same args returns the prior `QueryExecutionId` byte-for-byte;
    /// a replay with different args raises
    /// `IdempotentParameterMismatch`. Entries auto-expire after the
    /// cache's 24h TTL.
    pub start_query_idempotency: IdempotencyCache<Value>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AthenaStateSnapshot {
    #[serde(default)]
    pub workgroups: Vec<WorkGroup>,
    #[serde(default)]
    pub query_executions: Vec<QueryExecution>,
    #[serde(default)]
    pub named_queries: Vec<NamedQuery>,
    #[serde(default)]
    pub data_catalogs: Vec<DataCatalog>,
    #[serde(default)]
    pub prepared_statements: Vec<(String, PreparedStatement)>,
    #[serde(default)]
    pub resource_tags: Vec<(String, HashMap<String, String>)>,
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
                selected_engine_version: default_selected_engine(),
                effective_engine_version: default_effective_engine(),
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

    pub fn to_snapshot(&self) -> AthenaStateSnapshot {
        AthenaStateSnapshot {
            workgroups: self.workgroups.iter().map(|e| e.value().clone()).collect(),
            query_executions: self
                .query_executions
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            named_queries: self
                .named_queries
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            data_catalogs: self
                .data_catalogs
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            prepared_statements: self
                .prepared_statements
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
            resource_tags: self
                .resource_tags
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: AthenaStateSnapshot) {
        self.workgroups.clear();
        for w in snap.workgroups {
            self.workgroups.insert(w.name.clone(), w);
        }
        self.query_executions.clear();
        for q in snap.query_executions {
            self.query_executions.insert(q.id.clone(), q);
        }
        self.named_queries.clear();
        for n in snap.named_queries {
            self.named_queries.insert(n.id.clone(), n);
        }
        self.data_catalogs.clear();
        for d in snap.data_catalogs {
            self.data_catalogs.insert(d.name.clone(), d);
        }
        self.prepared_statements.clear();
        for (k, v) in snap.prepared_statements {
            self.prepared_statements.insert(k, v);
        }
        self.resource_tags.clear();
        for (k, v) in snap.resource_tags {
            self.resource_tags.insert(k, v);
        }
    }
}
