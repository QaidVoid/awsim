use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub arn: String,
    pub uri: String,
    pub location_type: String,
    pub config: Value,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub arn: String,
    pub name: String,
    pub status: String,
    pub source_location_arn: String,
    pub destination_location_arn: String,
    pub options: Value,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecution {
    pub arn: String,
    pub task_arn: String,
    pub status: String,
    pub started_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataSyncState {
    pub locations: DashMap<String, Location>,
    pub tasks: DashMap<String, Task>,
    pub executions: DashMap<String, TaskExecution>,
}
