use dashmap::DashMap;
use std::collections::HashMap;

/// An execution history event.
#[derive(Debug, Clone)]
pub struct HistoryEvent {
    pub id: u64,
    pub event_type: String,
    pub timestamp: String,
    pub details: serde_json::Value,
}

/// A Step Functions execution.
#[derive(Debug, Clone)]
pub struct Execution {
    pub arn: String,
    pub state_machine_arn: String,
    pub name: String,
    pub status: String, // RUNNING, SUCCEEDED, FAILED, TIMED_OUT, ABORTED
    pub input: String,
    pub output: Option<String>,
    pub start_date: String,
    pub stop_date: Option<String>,
    pub history: Vec<HistoryEvent>,
    pub error: Option<String>,
    pub cause: Option<String>,
}

/// A Step Functions state machine.
#[derive(Debug, Clone)]
pub struct StateMachine {
    pub name: String,
    pub arn: String,
    pub definition: String,
    pub role_arn: String,
    pub machine_type: String, // STANDARD or EXPRESS
    pub status: String,
    pub creation_date: String,
    /// Tags attached to this state machine.
    pub tags: HashMap<String, String>,
    /// `{ enabled: bool }`. Persisted verbatim and surfaced in describe.
    pub tracing_configuration: Option<serde_json::Value>,
    /// `{ type, kmsKeyId?, kmsDataKeyReusePeriodSeconds? }`. Type must
    /// be AWS_OWNED_KEY or CUSTOMER_MANAGED_KMS_KEY.
    pub encryption_configuration: Option<serde_json::Value>,
}

/// A Step Functions activity.
#[derive(Debug, Clone)]
pub struct Activity {
    pub name: String,
    pub arn: String,
    pub creation_date: String,
    pub tags: HashMap<String, String>,
}

/// Per-account/region Step Functions state.
#[derive(Debug, Default)]
pub struct StepFunctionsState {
    /// stateMachineArn → StateMachine
    pub state_machines: DashMap<String, StateMachine>,
    /// executionArn → Execution
    pub executions: DashMap<String, Execution>,
    /// activityArn → Activity
    pub activities: DashMap<String, Activity>,
}
