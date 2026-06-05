use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::asl;
use crate::operations::executions::epoch_secs;
use crate::operations::state_machines::ts_num;
use crate::state::{PendingTask, StepFunctionsState};

fn token_not_found(token: &str) -> AwsError {
    AwsError::not_found(
        "TaskDoesNotExist",
        format!("Task Token does not exist: {token}"),
    )
}

/// Apply a resumed execution result to the stored execution: append the
/// tail history and set the terminal status/output, or register a new
/// token and stay RUNNING when the tail hit another waitForTaskToken.
fn apply_resumed(state: &StepFunctionsState, pending: &PendingTask, resumed: asl::ExecResult) {
    if resumed.status == "WAITING"
        && let Some(token) = resumed.waiting_token.clone()
    {
        state.pending_tokens.insert(
            token,
            PendingTask {
                exec_arn: pending.exec_arn.clone(),
                definition: pending.definition.clone(),
                is_express: pending.is_express,
                waiting_state: resumed.waiting_state.clone().unwrap_or_default(),
                next_state: resumed.waiting_next.clone(),
                input_at_wait: resumed
                    .waiting_input
                    .clone()
                    .unwrap_or_else(|| "{}".to_string()),
                result_path: resumed.waiting_result_path.clone(),
                start_date: pending.start_date.clone(),
                last_heartbeat: epoch_secs(),
            },
        );
    }

    if let Some(mut exec) = state.executions.get_mut(&pending.exec_arn) {
        exec.history.extend(resumed.history);
        if resumed.status == "WAITING" {
            exec.status = "RUNNING".to_string();
        } else {
            exec.status = resumed.status;
            exec.output = resumed.output;
            exec.error = resumed.error;
            exec.cause = resumed.cause;
            exec.stop_date = Some(epoch_secs().to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// SendTaskSuccess
// ---------------------------------------------------------------------------

pub fn send_task_success(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;
    let output = input["output"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "output is required"))?;

    let (_k, pending) = state
        .pending_tokens
        .remove(token)
        .ok_or_else(|| token_not_found(token))?;
    let output_val: Value = serde_json::from_str(output).unwrap_or(Value::Null);
    let resumed = asl::resume_execution_success(
        &pending.definition,
        &pending.start_date,
        pending.is_express,
        pending.next_state.as_deref(),
        &pending.input_at_wait,
        pending.result_path.as_deref(),
        output_val,
    );
    apply_resumed(state, &pending, resumed);
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// SendTaskFailure
// ---------------------------------------------------------------------------

pub fn send_task_failure(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;
    let error = input["error"].as_str().unwrap_or("States.TaskFailed");
    let cause = input["cause"].as_str().unwrap_or("");

    let (_k, pending) = state
        .pending_tokens
        .remove(token)
        .ok_or_else(|| token_not_found(token))?;
    let resumed = asl::resume_execution_failure(
        &pending.definition,
        &pending.start_date,
        pending.is_express,
        &pending.waiting_state,
        &pending.input_at_wait,
        error,
        cause,
    );
    apply_resumed(state, &pending, resumed);
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// SendTaskHeartbeat
// ---------------------------------------------------------------------------

pub fn send_task_heartbeat(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["taskToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "taskToken is required"))?;
    let mut pending = state
        .pending_tokens
        .get_mut(token)
        .ok_or_else(|| token_not_found(token))?;
    pending.last_heartbeat = epoch_secs();
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
        "creationDate": ts_num(&sm.creation_date),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::executions::start_execution;
    use crate::operations::state_machines::create_state_machine;

    fn ctx() -> RequestContext {
        RequestContext::new("states", "us-east-1")
    }

    const WAIT_DEF: &str = r#"{
        "StartAt": "Wait",
        "States": {
            "Wait": { "Type": "Task", "Resource": "arn:aws:states:::lambda:invoke.waitForTaskToken", "Next": "Done" },
            "Done": { "Type": "Pass", "End": true }
        }
    }"#;

    const WAIT_CATCH_DEF: &str = r#"{
        "StartAt": "Wait",
        "States": {
            "Wait": { "Type": "Task", "Resource": "arn:aws:states:::lambda:invoke.waitForTaskToken",
                      "Catch": [{ "ErrorEquals": ["States.ALL"], "Next": "Recover" }], "Next": "Done" },
            "Recover": { "Type": "Pass", "End": true },
            "Done": { "Type": "Pass", "End": true }
        }
    }"#;

    fn start(state: &StepFunctionsState, def: &str) -> (String, String) {
        let created = create_state_machine(
            state,
            &json!({ "name": "m", "definition": def, "roleArn": "arn:aws:iam::000000000000:role/r" }),
            &ctx(),
        )
        .unwrap();
        let sm_arn = created["stateMachineArn"].as_str().unwrap().to_string();
        start_execution(
            state,
            &json!({ "stateMachineArn": sm_arn, "name": "e1", "input": "{\"x\":1}" }),
            &ctx(),
        )
        .unwrap();
        let exec_arn = state.executions.iter().next().unwrap().key().clone();
        let token = state.pending_tokens.iter().next().unwrap().key().clone();
        (exec_arn, token)
    }

    #[test]
    fn wait_blocks_then_success_resumes() {
        let state = StepFunctionsState::default();
        let (exec_arn, token) = start(&state, WAIT_DEF);
        assert_eq!(state.executions.get(&exec_arn).unwrap().status, "RUNNING");
        send_task_success(
            &state,
            &json!({ "taskToken": token, "output": "{\"done\":true}" }),
            &ctx(),
        )
        .unwrap();
        let exec = state.executions.get(&exec_arn).unwrap();
        assert_eq!(exec.status, "SUCCEEDED");
        assert!(exec.output.as_ref().unwrap().contains("done"));
        assert!(state.pending_tokens.is_empty());
    }

    #[test]
    fn failure_routes_through_catch() {
        let state = StepFunctionsState::default();
        let (exec_arn, token) = start(&state, WAIT_CATCH_DEF);
        send_task_failure(
            &state,
            &json!({ "taskToken": token, "error": "MyError", "cause": "boom" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(state.executions.get(&exec_arn).unwrap().status, "SUCCEEDED");
    }

    #[test]
    fn failure_uncaught_fails_execution() {
        let state = StepFunctionsState::default();
        let (exec_arn, token) = start(&state, WAIT_DEF);
        send_task_failure(
            &state,
            &json!({ "taskToken": token, "error": "MyError", "cause": "boom" }),
            &ctx(),
        )
        .unwrap();
        let exec = state.executions.get(&exec_arn).unwrap();
        assert_eq!(exec.status, "FAILED");
        assert_eq!(exec.error.as_deref(), Some("MyError"));
    }

    #[test]
    fn heartbeat_keeps_running_and_unknown_token_errors() {
        let state = StepFunctionsState::default();
        let (exec_arn, token) = start(&state, WAIT_DEF);
        send_task_heartbeat(&state, &json!({ "taskToken": token }), &ctx()).unwrap();
        assert_eq!(state.executions.get(&exec_arn).unwrap().status, "RUNNING");
        assert!(state.pending_tokens.contains_key(&token));

        let err = send_task_success(
            &state,
            &json!({ "taskToken": "nope", "output": "{}" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "TaskDoesNotExist");
        let err = send_task_heartbeat(&state, &json!({ "taskToken": "nope" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "TaskDoesNotExist");
    }
}
