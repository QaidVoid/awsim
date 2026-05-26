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
    /// AWS ECS `loadBalancers[]` (one of `{targetGroupArn|loadBalancerName, containerName, containerPort}` per entry).
    /// Persisted verbatim and echoed on describe.
    pub load_balancers: Vec<Value>,
    /// `{ minimumHealthyPercent, maximumPercent, deploymentCircuitBreaker, alarms }`.
    pub deployment_configuration: Option<Value>,
    /// `{ type: ECS|CODE_DEPLOY|EXTERNAL }`. Validated at CreateService.
    pub deployment_controller: Option<Value>,
    /// `{ awsvpcConfiguration: { subnets, securityGroups, assignPublicIp } }`.
    pub network_configuration: Option<Value>,
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
    pub capacity_providers: Vec<String>,
    pub default_capacity_provider_strategy: Vec<Value>,
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
    /// Task-level CPU as a string (Fargate uses "256".."16384"; EC2
    /// supports CPU shares 0-10240). Stored verbatim so DescribeTaskDefinition
    /// echoes what the caller registered.
    pub cpu: Option<String>,
    /// Task-level memory in MiB as a string (Fargate uses fixed pairs
    /// with cpu; EC2 is any positive integer).
    pub memory: Option<String>,
    /// Per-task placementConstraints. Each entry is `{ type, expression }`;
    /// `type` is one of `memberOf` or `distinctInstance`. Stored verbatim.
    pub placement_constraints: Vec<Value>,
    /// Per-task placementStrategy. Each entry is `{ type, field }`;
    /// `type` is one of `random`, `spread`, or `binpack`. Stored verbatim.
    pub placement_strategy: Vec<Value>,
    /// Top-level `volumes` declared on the task definition (no real
    /// mount — entries are stored verbatim so DescribeTaskDefinition
    /// echoes the same shape the caller registered).
    pub volumes: Vec<Value>,
}

/// A capacity provider.
#[derive(Debug, Clone)]
pub struct CapacityProvider {
    pub name: String,
    pub arn: String,
    pub status: String,
}

/// Per-account/region ECS state.
#[derive(Debug, Default)]
pub struct EcsState {
    /// cluster name → Cluster
    pub clusters: DashMap<String, Cluster>,
    /// family → ordered Vec of TaskDefinition (index 0 = revision 1)
    pub task_definitions: DashMap<String, Vec<TaskDefinition>>,
    /// resource ARN → HashMap<tag key, tag value>
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    /// capacity provider name → CapacityProvider
    pub capacity_providers: DashMap<String, CapacityProvider>,
    /// account setting name → value (e.g. "containerInstanceLongArnFormat" → "enabled")
    pub account_settings: DashMap<String, String>,
    /// "{cluster_name}|{target_type}" → map of attribute name → value
    pub attributes: DashMap<String, HashMap<String, String>>,
}
