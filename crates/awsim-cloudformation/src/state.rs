use dashmap::DashMap;
use std::collections::HashMap;

/// CloudFormation state — per account+region.
#[derive(Debug, Default)]
pub struct CloudFormationState {
    pub stacks: DashMap<String, Stack>,
}

#[derive(Debug, Clone)]
pub struct Stack {
    pub stack_id: String,
    pub stack_name: String,
    pub template_body: String,
    pub parameters: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    /// e.g. CREATE_COMPLETE, UPDATE_COMPLETE, DELETE_COMPLETE, ROLLBACK_COMPLETE
    pub status: String,
    pub status_reason: Option<String>,
    pub resources: Vec<StackResource>,
    pub events: Vec<StackEvent>,
    pub change_sets: HashMap<String, ChangeSet>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub outputs: HashMap<String, StackOutput>,
}

#[derive(Debug, Clone)]
pub struct StackResource {
    pub logical_resource_id: String,
    pub physical_resource_id: Option<String>,
    pub resource_type: String,
    pub resource_status: String,
    pub resource_status_reason: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct StackEvent {
    pub event_id: String,
    pub stack_id: String,
    pub stack_name: String,
    pub logical_resource_id: String,
    pub physical_resource_id: Option<String>,
    pub resource_type: String,
    pub timestamp: String,
    pub resource_status: String,
    pub resource_status_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChangeSet {
    pub change_set_id: String,
    pub change_set_name: String,
    pub stack_id: String,
    pub stack_name: String,
    pub template_body: Option<String>,
    pub parameters: HashMap<String, String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub changes: Vec<Change>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub action: String,
    pub logical_resource_id: String,
    pub resource_type: String,
}

#[derive(Debug, Clone)]
pub struct StackOutput {
    pub output_key: String,
    pub output_value: String,
    pub description: Option<String>,
}
