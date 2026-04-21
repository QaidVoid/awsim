use dashmap::DashMap;

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
}

/// Per-account/region Step Functions state.
#[derive(Debug, Default)]
pub struct StepFunctionsState {
    /// stateMachineArn → StateMachine
    pub state_machines: DashMap<String, StateMachine>,
    /// executionArn → Execution
    pub executions: DashMap<String, Execution>,
}
