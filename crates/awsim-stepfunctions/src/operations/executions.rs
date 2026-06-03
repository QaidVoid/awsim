use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::asl;
use crate::state::{Execution, PendingTask, StepFunctionsState};

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

pub(crate) fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
    let logging_configuration = sm.logging_configuration.clone();
    drop(sm); // release dashmap reference before potentially mutating

    let result = asl::run_execution(&definition, &exec_input, &start_date, is_express)?;

    // A `.waitForTaskToken` Task suspended the run: register the token so a
    // SendTaskSuccess/Failure callback can resume, and keep the execution
    // RUNNING (AWS reports waiting executions as RUNNING).
    let is_waiting = result.status == "WAITING";
    if is_waiting && let Some(token) = result.waiting_token.clone() {
        state.pending_tokens.insert(
            token,
            PendingTask {
                exec_arn: exec_arn.clone(),
                definition: definition.clone(),
                is_express,
                waiting_state: result.waiting_state.clone().unwrap_or_default(),
                next_state: result.waiting_next.clone(),
                input_at_wait: result
                    .waiting_input
                    .clone()
                    .unwrap_or_else(|| "{}".to_string()),
                result_path: result.waiting_result_path.clone(),
                start_date: start_date.clone(),
                last_heartbeat: epoch_secs(),
            },
        );
    }

    let stored_status = if is_waiting {
        "RUNNING".to_string()
    } else {
        result.status.clone()
    };
    let terminal = stored_status != "RUNNING";

    let exec = Execution {
        arn: exec_arn.clone(),
        state_machine_arn: sm_arn.to_string(),
        name: exec_name,
        status: stored_status,
        input: exec_input,
        output: if is_waiting { None } else { result.output },
        start_date: start_date.clone(),
        stop_date: if terminal { Some(now_iso8601()) } else { None },
        history: result.history,
        error: if is_waiting { None } else { result.error },
        cause: if is_waiting { None } else { result.cause },
    };

    info!(arn = %exec_arn, status = %exec.status, "Started execution");

    // Export execution history to CloudWatch Logs when logging is enabled.
    // The binary's event router creates the log group/stream and writes
    // the events; best-effort, skipped in unit tests (no event bus).
    if let (Some(bus), Some(cfg)) = (ctx.event_bus.as_ref(), logging_configuration.as_ref()) {
        let level = cfg.get("level").and_then(Value::as_str).unwrap_or("OFF");
        let log_group_arn = cfg
            .get("destinations")
            .and_then(Value::as_array)
            .and_then(|d| d.first())
            .and_then(|d| d.get("cloudWatchLogsLogGroup"))
            .and_then(|g| g.get("logGroupArn"))
            .and_then(Value::as_str);
        if level != "OFF"
            && let Some(arn) = log_group_arn
        {
            let events: Vec<Value> = exec
                .history
                .iter()
                .map(|e| json!({ "type": e.event_type, "id": e.id, "timestamp": e.timestamp }))
                .collect();
            bus.publish(awsim_core::events::InternalEvent {
                source: "states".to_string(),
                event_type: "states:ExecutionLog".to_string(),
                region: ctx.region.clone(),
                account_id: ctx.account_id.clone(),
                detail: json!({
                    "logGroupArn": arn,
                    "executionArn": exec.arn,
                    "name": exec.name,
                    "status": exec.status,
                    "events": events,
                }),
            });
        }
    }

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

    let max_results = cap_max_results(input["maxResults"].as_i64(), 100, 1000);
    let mut items: Vec<(String, Value)> = state
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
            (
                exec.arn.clone(),
                json!({
                    "executionArn": exec.arn,
                    "stateMachineArn": exec.state_machine_arn,
                    "name": exec.name,
                    "status": exec.status,
                    "startDate": exec.start_date,
                }),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let page = paginate(items, max_results, input["nextToken"].as_str(), |(k, _)| {
        k.clone()
    })?;
    let executions: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    let mut resp = json!({ "executions": executions });
    if let Some(token) = page.next_token {
        resp["nextToken"] = json!(token);
    }
    Ok(resp)
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

#[cfg(test)]
mod logging_emit_tests {
    use super::*;
    use crate::operations::state_machines::create_state_machine;

    #[test]
    fn start_execution_emits_states_execution_log_when_logging_enabled() {
        let state = StepFunctionsState::default();
        let setup_ctx = RequestContext::new("states", "us-east-1");
        let created = create_state_machine(
            &state,
            &json!({
                "name": "m",
                "definition": r#"{"StartAt":"X","States":{"X":{"Type":"Pass","End":true}}}"#,
                "roleArn": "arn:aws:iam::000000000000:role/r",
                "loggingConfiguration": {
                    "level": "ALL",
                    "destinations": [{ "cloudWatchLogsLogGroup": {
                        "logGroupArn": "arn:aws:logs:us-east-1:000000000000:log-group:/sfn:*"
                    }}],
                },
            }),
            &setup_ctx,
        )
        .unwrap();
        let sm_arn = created["stateMachineArn"].as_str().unwrap().to_string();

        let bus = awsim_core::events::EventBus::new();
        let mut rx = bus.subscribe();
        let mut ctx = RequestContext::new("states", "us-east-1");
        ctx.event_bus = Some(bus);
        start_execution(
            &state,
            &json!({ "stateMachineArn": sm_arn, "name": "e1", "input": "{}" }),
            &ctx,
        )
        .unwrap();
        let ev = rx.try_recv().expect("expected a states:ExecutionLog");
        assert_eq!(ev.event_type, "states:ExecutionLog");
        assert!(
            ev.detail["logGroupArn"]
                .as_str()
                .unwrap()
                .contains("log-group:/sfn")
        );
        assert_eq!(ev.detail["status"], "SUCCEEDED");
    }

    #[test]
    fn start_execution_without_logging_emits_nothing() {
        let state = StepFunctionsState::default();
        let setup_ctx = RequestContext::new("states", "us-east-1");
        let created = create_state_machine(
            &state,
            &json!({
                "name": "m",
                "definition": r#"{"StartAt":"X","States":{"X":{"Type":"Pass","End":true}}}"#,
                "roleArn": "arn:aws:iam::000000000000:role/r",
            }),
            &setup_ctx,
        )
        .unwrap();
        let sm_arn = created["stateMachineArn"].as_str().unwrap().to_string();
        let bus = awsim_core::events::EventBus::new();
        let mut rx = bus.subscribe();
        let mut ctx = RequestContext::new("states", "us-east-1");
        ctx.event_bus = Some(bus);
        start_execution(
            &state,
            &json!({ "stateMachineArn": sm_arn, "name": "e1", "input": "{}" }),
            &ctx,
        )
        .unwrap();
        assert!(rx.try_recv().is_err());
    }
}
