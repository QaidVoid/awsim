use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
    /// Partitions keyed by partition values string (e.g. "2024-01-01/us-east-1")
    pub partitions: Vec<GluePartition>,
}

/// A Glue partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GluePartition {
    pub values: Vec<String>,
    pub storage_descriptor: Option<Value>,
    pub created_at: String,
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
    pub schedule: Option<String>,
    pub description: Option<String>,
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

/// A Glue job run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRun {
    pub id: String,
    pub job_name: String,
    pub status: String,
    pub started_on: String,
    pub completed_on: Option<String>,
    pub arguments: Option<Value>,
}

/// A Glue connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub connection_type: String,
    pub connection_properties: HashMap<String, String>,
    pub description: Option<String>,
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
    /// run_id → JobRun
    pub job_runs: DashMap<String, JobRun>,
    /// connection_name → Connection
    pub connections: DashMap<String, Connection>,
    /// resource_arn → tags
    pub tags: DashMap<String, HashMap<String, String>>,
}
