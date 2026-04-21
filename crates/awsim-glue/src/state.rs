use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A Glue Data Catalog database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlueDatabase {
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
}

/// A Glue table stored under "db_name.table_name".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlueTable {
    pub database_name: String,
    pub name: String,
    /// StorageDescriptor, etc. stored as raw JSON for flexibility.
    pub storage_descriptor: Option<Value>,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A Glue crawler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crawler {
    pub name: String,
    pub role: String,
    pub database_name: Option<String>,
    pub targets: Option<Value>,
    /// READY | RUNNING | STOPPING
    pub state: String,
    pub created_at: String,
}

/// A Glue ETL job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub name: String,
    pub role: String,
    pub command: Option<Value>,
    pub default_arguments: Option<Value>,
    pub created_at: String,
}

/// Per-account/region Glue state.
#[derive(Debug, Default)]
pub struct GlueState {
    /// Database name → GlueDatabase
    pub databases: DashMap<String, GlueDatabase>,
    /// "db_name.table_name" → GlueTable
    pub tables: DashMap<String, GlueTable>,
    /// Crawler name → Crawler
    pub crawlers: DashMap<String, Crawler>,
    /// Job name → Job
    pub jobs: DashMap<String, Job>,
}
