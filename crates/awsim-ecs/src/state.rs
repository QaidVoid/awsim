use std::collections::HashMap;

use dashmap::DashMap;
use serde_json::Value;

/// A task running in a cluster (does not actually run containers).
#[derive(Debug, Clone)]
pub struct Task {
    pub task_arn: String,
    pub cluster_arn: String,
    pub task_definition_arn: String,
    pub status: String,
    pub started_at: String,
    pub group: String,
}

/// A service running in a cluster.
#[derive(Debug, Clone)]
pub struct Service {
    pub service_name: String,
    pub service_arn: String,
    pub cluster_arn: String,
    pub task_definition: String,
    pub desired_count: i64,
    pub running_count: i64,
    pub status: String,
    pub launch_type: String,
    pub created_at: String,
}

/// An ECS cluster.
#[derive(Debug)]
pub struct Cluster {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub services: HashMap<String, Service>,
    pub tasks: HashMap<String, Task>,
    #[allow(dead_code)]
    pub created_at: String,
}

/// A task definition revision.
#[derive(Debug, Clone)]
pub struct TaskDefinition {
    pub family: String,
    pub revision: u32,
    pub arn: String,
    pub container_definitions: Value,
    pub status: String,
    pub network_mode: String,
    pub requires_compatibilities: Vec<String>,
}

/// Per-account/region ECS state.
#[derive(Debug, Default)]
pub struct EcsState {
    /// cluster name → Cluster
    pub clusters: DashMap<String, Cluster>,
    /// family → ordered Vec of TaskDefinition (index 0 = revision 1)
    pub task_definitions: DashMap<String, Vec<TaskDefinition>>,
}
