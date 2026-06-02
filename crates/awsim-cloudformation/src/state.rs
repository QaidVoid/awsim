use dashmap::DashMap;
use std::collections::HashMap;

/// CloudFormation state — per account+region.
#[derive(Debug, Default)]
pub struct CloudFormationState {
    pub stacks: DashMap<String, Stack>,
    /// stack name → HashMap<tag key, tag value> (for TagResource/UntagResource)
    pub stack_tags: DashMap<String, HashMap<String, String>>,
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
    /// When true, DeleteStack returns ValidationError until the caller
    /// flips it off via UpdateTerminationProtection. Mirrors AWS's
    /// stack-level safeguard against accidental deletes.
    pub termination_protection: bool,
    /// SNS topic ARNs that receive a `cloudformation:StackEvent`
    /// notification on every stack-status transition. AWS supports
    /// up to 5 ARNs; we accept the same upper bound.
    pub notification_arns: Vec<String>,
    /// `DO_NOTHING` | `ROLLBACK` | `DELETE`. AWS's default is
    /// `ROLLBACK`. The simulator never fails CreateStack, so this is
    /// stored verbatim for describe round-trip and consulted by the
    /// rollback path when (future) failures are wired up.
    pub on_failure: String,
    /// Optional stack policy document (JSON). When set, UpdateStack
    /// evaluates each resource change against it and blocks updates
    /// the policy denies. AWS surfaces this as `ValidationError`.
    pub stack_policy_body: Option<String>,
    /// Absolute unix-seconds deadline from `TimeoutInMinutes`. When a
    /// resource is still `CREATE_IN_PROGRESS` past this deadline, the
    /// tick driver rolls the stack back per `on_failure`.
    pub timeout_deadline_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StackResource {
    pub logical_resource_id: String,
    pub physical_resource_id: Option<String>,
    pub resource_type: String,
    pub resource_status: String,
    pub resource_status_reason: Option<String>,
    pub timestamp: String,
    /// `Delete` (default), `Retain`, `Snapshot`, or
    /// `RetainExceptOnCreate`. Drives DeleteStack behavior; AWS keeps
    /// retained resources around as the stack moves to DELETE_COMPLETE
    /// and surfaces them with `DELETE_SKIPPED` status.
    pub deletion_policy: Option<String>,
    /// Signals this resource must receive (via SignalResource) before it
    /// leaves `CREATE_IN_PROGRESS`. Set from
    /// `CreationPolicy.ResourceSignal.Count`, or 1 for a custom resource;
    /// 0 means the resource completes immediately.
    pub required_signal_count: u32,
    /// SUCCESS signals received so far.
    pub received_signal_count: u32,
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
    /// `True`, `False`, or `Conditional` — only set for `Modify`
    /// actions. AWS computes this from per-resource-type property
    /// metadata; AWSim uses a conservative heuristic (any non-tag
    /// property change implies replacement) since the simulator
    /// doesn't carry the per-resource property schema.
    pub replacement: Option<String>,
    /// Scope of the change: any combination of `Properties`,
    /// `Metadata`, `Tags`, `CreationPolicy`, `UpdatePolicy`, and
    /// `DeletionPolicy`. Empty for `Add` / `Remove` actions, since
    /// AWS leaves Scope null in those cases.
    pub scope: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StackOutput {
    pub output_key: String,
    pub output_value: String,
    pub description: Option<String>,
}
