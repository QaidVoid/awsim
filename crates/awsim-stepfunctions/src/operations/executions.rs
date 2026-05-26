use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::asl;
use crate::state::{Execution, StepFunctionsState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn build_exec_arn(ctx: &RequestContext, sm_name: &str, exec_name: &str) -> String {
    format!(
        "arn:aws:states:{}:{}:execution:{}:{}",
        ctx.region, ctx.account_id, sm_name, exec_name
    )
}

fn execution_to_value(exec: &Execution) -> Value {
    let mut v = json!({
        "executionArn": exec.arn,
        "stateMachineArn": exec.state_machine_arn,
        "name": exec.name,
        "status": exec.status,
        "startDate": exec.start_date,
        "input": exec.input,
    });

    if let Some(output) = &exec.output {
        v["output"] = json!(output);
    }
    if let Some(stop_date) = &exec.stop_date {
        v["stopDate"] = json!(stop_date);
    }
    if let Some(error) = &exec.error {
        v["error"] = json!(error);
    }
    if let Some(cause) = &exec.cause {
        v["cause"] = json!(cause);
    }

    v
}

// ---------------------------------------------------------------------------
// StartExecution
// ---------------------------------------------------------------------------

pub fn start_execution(
    state: &StepFunctionsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sm_arn = input["stateMachineArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "stateMachineArn is required"))?;

    let sm = state.state_machines.get(sm_arn).ok_or_else(|| {
        AwsError::not_found(
            "StateMachineDoesNotExist",
            format!("State machine not found: {sm_arn}"),
        )
    })?;

    let exec_name = input["name"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let exec_input = input["input"].as_str().unwrap_or("{}").to_string();

    // Extract state machine name from ARN (last segment after "stateMachine:")
    let sm_name = sm_arn.rsplit(':').next().unwrap_or("unknown");

    let exec_arn = build_exec_arn(ctx, sm_name, &exec_name);

    if state.executions.contains_key(&exec_arn) {
        return Err(AwsError::conflict(
            "ExecutionAlreadyExists",
            format!("Execution already exists: {exec_arn}"),
        ));
    }

    let start_date = now_iso8601();

    // Run the ASL interpreter synchronously (dev emulator)
    let definition = sm.definition.clone();
    let is_express = sm.machine_type == "EXPRESS";
    drop(sm); // release dashmap reference before potentially mutating

    let result = asl::run_execution(&definition, &exec_input, &start_date, is_express)?;

    let exec = Execution {
        arn: exec_arn.clone(),
        state_machine_arn: sm_arn.to_string(),
        name: exec_name,
        status: result.status.clone(),
        input: exec_input,
        output: result.output,
        start_date: start_date.clone(),
        stop_date: if result.status != "RUNNING" {
            Some(now_iso8601())
        } else {
            None
        },
        history: result.history,
        error: result.error,
        cause: result.cause,
    };

    info!(arn = %exec_arn, status = %exec.status, "Started execution");

    // AWS bills Step Functions per state transition, not per
    // StartExecution call. Each state we entered counts as one
    // billable transition; expose the count via an internal metadata
    // header so the billing meter can charge accurately.
    let state_transitions = exec
        .history
        .iter()
        .filter(|e| e.event_type == "StateEntered")
        .count() as u32;

    state.executions.insert(exec_arn.clone(), exec);

    Ok(json!({
        "executionArn": exec_arn,
        "startDate": start_date,
        "__headers": {
            "X-Awsim-State-Transitions": state_transitions.to_string(),
        },
    }))
}

// ---------------------------------------------------------------------------
// StopExecution
// ---------------------------------------------------------------------------

pub fn stop_execution(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let exec_arn = input["executionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "executionArn is required"))?;

    let mut exec = state.executions.get_mut(exec_arn).ok_or_else(|| {
        AwsError::not_found(
            "ExecutionDoesNotExist",
            format!("Execution not found: {exec_arn}"),
        )
    })?;

    if exec.status != "RUNNING" {
        return Err(AwsError::bad_request(
            "InvalidExecutionStatus",
            format!("Execution is not in RUNNING state: {}", exec.status),
        ));
    }

    exec.status = "ABORTED".to_string();
    exec.stop_date = Some(now_iso8601());
    exec.error = input["error"].as_str().map(|s| s.to_string());
    exec.cause = input["cause"].as_str().map(|s| s.to_string());

    let stop_date = exec.stop_date.clone().unwrap_or_default();
    info!(arn = %exec_arn, "Stopped execution");

    Ok(json!({ "stopDate": stop_date }))
}

// ---------------------------------------------------------------------------
// DescribeExecution
// ---------------------------------------------------------------------------

pub fn describe_execution(
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

    Ok(execution_to_value(&exec))
}

// ---------------------------------------------------------------------------
// ListExecutions
// ---------------------------------------------------------------------------

pub fn list_executions(
    state: &StepFunctionsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sm_arn = input["stateMachineArn"].as_str();
    let status_filter = input["statusFilter"].as_str();

    let executions: Vec<Value> = state
        .executions
        .iter()
        .filter(|entry| {
            let exec = entry.value();
            if let Some(arn) = sm_arn
                && exec.state_machine_arn != arn
            {
                return false;
            }
            if let Some(status) = status_filter
                && exec.status != status
            {
                return false;
            }
            true
        })
        .map(|entry| {
            let exec = entry.value();
            json!({
                "executionArn": exec.arn,
                "stateMachineArn": exec.state_machine_arn,
                "name": exec.name,
                "status": exec.status,
                "startDate": exec.start_date,
            })
        })
        .collect();

    Ok(json!({ "executions": executions }))
}

// ---------------------------------------------------------------------------
// GetExecutionHistory
// ---------------------------------------------------------------------------

pub fn get_execution_history(
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

    let events: Vec<Value> = exec
        .history
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "type": e.event_type,
                "timestamp": e.timestamp,
                "details": e.details,
            })
        })
        .collect();

    Ok(json!({ "events": events }))
}
