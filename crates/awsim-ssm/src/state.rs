use std::collections::HashMap;

use dashmap::DashMap;

/// A single version entry for a parameter.
#[derive(Debug, Clone)]
pub struct ParameterVersion {
    pub value: String,
    pub version: u64,
    /// Unix epoch seconds (stored as u64, serialised as a JSON number).
    pub date: u64,
    pub description: String,
    /// Labels attached to this version.
    pub labels: Vec<String>,
}

/// A stored SSM parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub arn: String,
    pub param_type: String, // String, StringList, SecureString
    pub value: String,
    pub description: String,
    pub version: u64,
    /// Unix epoch seconds (stored as u64, serialised as a JSON number).
    pub last_modified_date: u64,
    pub tags: HashMap<String, String>,
    pub history: Vec<ParameterVersion>,
    pub tier: String,
    /// Labels on the current version.
    pub labels: Vec<String>,
}

/// A stored SSM Run Command record (stub).
#[derive(Debug, Clone)]
pub struct Command {
    pub command_id: String,
    pub document_name: String,
    pub targets: Vec<serde_json::Value>,
    pub status: String,
    pub created_time: u64,
}

/// An SSM Document.
#[derive(Debug, Clone)]
pub struct SsmDocument {
    pub name: String,
    #[allow(dead_code)]
    pub arn: String,
    pub document_type: String,
    pub document_format: String,
    pub content: String,
    pub status: String,
    pub document_version: String,
    pub created_date: u64,
}

/// An SSM State Manager Association.
#[derive(Debug, Clone)]
pub struct SsmAssociation {
    pub association_id: String,
    #[allow(dead_code)]
    pub name: String,
    pub document_name: String,
    pub targets: Vec<serde_json::Value>,
    pub status: String,
    pub created_date: u64,
}

/// An SSM Maintenance Window stub.
#[derive(Debug, Clone)]
pub struct SsmMaintenanceWindow {
    pub window_id: String,
    pub name: String,
    pub schedule: String,
    pub duration: u64,
    pub cutoff: u64,
    pub enabled: bool,
    #[allow(dead_code)]
    pub created_date: u64,
}

/// An SSM OpsCenter OpsItem.
#[derive(Debug, Clone)]
pub struct SsmOpsItem {
    pub ops_item_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub severity: String,
    pub created_time: u64,
    pub last_modified_time: u64,
}

#[derive(Debug, Clone)]
pub struct SsmPatchBaseline {
    pub baseline_id: String,
    pub name: String,
    pub operating_system: String,
    pub description: String,
    pub approved_patches: Vec<String>,
    pub rejected_patches: Vec<String>,
    pub created_date: u64,
    pub modified_date: u64,
}

#[derive(Debug, Clone)]
pub struct SsmAutomationExecution {
    pub execution_id: String,
    pub document_name: String,
    pub document_version: String,
    pub status: String,
    pub mode: String,
    pub parameters: serde_json::Value,
    pub started_time: u64,
    pub end_time: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SsmSession {
    pub session_id: String,
    pub target: String,
    pub status: String,
    pub document_name: String,
    pub start_date: u64,
    pub end_date: Option<u64>,
    pub owner: String,
}

#[derive(Debug, Clone)]
pub struct SsmMaintenanceWindowTarget {
    pub window_target_id: String,
    pub window_id: String,
    pub resource_type: String,
    pub targets: Vec<serde_json::Value>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SsmMaintenanceWindowTask {
    pub window_task_id: String,
    pub window_id: String,
    pub task_arn: String,
    pub task_type: String,
    pub targets: Vec<serde_json::Value>,
    pub priority: u64,
    pub max_concurrency: String,
    pub max_errors: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SsmResourceDataSync {
    pub sync_name: String,
    pub sync_type: String,
    pub s3_bucket_name: String,
    pub s3_region: String,
    pub s3_prefix: String,
    pub last_sync_time: u64,
    pub sync_created_time: u64,
}

/// Per-account/region SSM state.
#[derive(Debug, Default)]
pub struct SsmState {
    /// Parameter name → Parameter
    pub parameters: DashMap<String, Parameter>,
    /// CommandId → Command
    pub commands: DashMap<String, Command>,
    /// Document name → SsmDocument
    pub documents: DashMap<String, SsmDocument>,
    /// AssociationId → SsmAssociation
    pub associations: DashMap<String, SsmAssociation>,
    /// WindowId → SsmMaintenanceWindow
    pub maintenance_windows: DashMap<String, SsmMaintenanceWindow>,
    /// OpsItemId → SsmOpsItem
    pub ops_items: DashMap<String, SsmOpsItem>,
    pub patch_baselines: DashMap<String, SsmPatchBaseline>,
    pub default_patch_baselines: DashMap<String, String>,
    pub automation_executions: DashMap<String, SsmAutomationExecution>,
    pub sessions: DashMap<String, SsmSession>,
    pub maintenance_window_targets: DashMap<String, SsmMaintenanceWindowTarget>,
    pub maintenance_window_tasks: DashMap<String, SsmMaintenanceWindowTask>,
    pub resource_data_syncs: DashMap<String, SsmResourceDataSync>,
}
