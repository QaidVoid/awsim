use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::StepFunctionsState;

// ---------------------------------------------------------------------------
// SendTaskSuccess
// ---------------------------------------------------------------------------

pub fn send_task_success(
    _state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // taskToken is required by the API
    input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;

    // output must be provided
    input["output"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "output is required"))?;

    // In a dev emulator we silently accept the callback.
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// SendTaskFailure
// ---------------------------------------------------------------------------

pub fn send_task_failure(
    _state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// SendTaskHeartbeat
// ---------------------------------------------------------------------------

pub fn send_task_heartbeat(
    _state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeStateMachineForExecution
// ---------------------------------------------------------------------------

pub fn describe_state_machine_for_execution(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let exec_arn = input["executionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "executionArn is required"))?;

    let exec = state.executions.get(exec_arn).ok_or_else(|| {
        AwsError::not_found(
            "ExecutionDoesNotExist",
            format!("Execution not found: {exec_arn}"),
        )
    })?;

    let sm_arn = exec.state_machine_arn.clone();
    drop(exec);

    let sm = state.state_machines.get(&sm_arn).ok_or_else(|| {
        AwsError::not_found(
            "StateMachineDoesNotExist",
            format!("State machine not found: {sm_arn}"),
        )
    })?;

    Ok(json!({
        "stateMachineArn": sm.arn,
        "name": sm.name,
        "status": sm.status,
        "definition": sm.definition,
        "roleArn": sm.role_arn,
        "type": sm.machine_type,
        "creationDate": sm.creation_date,
    }))
}
