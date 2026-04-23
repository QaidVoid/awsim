use dashmap::DashMap;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ComputeEnvironment {
    pub name: String,
    pub arn: String,
    pub env_type: String,
    pub state: String,
    pub status: String,
    pub compute_resources: Value,
    pub service_role: String,
}

#[derive(Debug, Clone)]
pub struct JobQueue {
    pub name: String,
    pub arn: String,
    pub state: String,
    pub status: String,
    pub priority: u64,
    pub compute_environment_order: Value,
}

#[derive(Debug, Clone)]
pub struct JobDefinition {
    pub name: String,
    pub arn: String,
    pub revision: u64,
    pub job_type: String,
    pub container_properties: Value,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub queue: String,
    pub definition: String,
    pub status: String,
    pub created_at: u64,
}

#[derive(Debug, Default)]
pub struct BatchState {
    pub compute_environments: DashMap<String, ComputeEnvironment>,
    pub job_queues: DashMap<String, JobQueue>,
    pub job_definitions: DashMap<String, JobDefinition>,
    pub job_definition_revisions: DashMap<String, u64>,
    pub jobs: DashMap<String, Job>,
}
