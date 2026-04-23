use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmAutomationExecution, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn execution_summary(e: &SsmAutomationExecution) -> Value {
    json!({
        "AutomationExecutionId": e.execution_id,
        "DocumentName": e.document_name,
        "DocumentVersion": e.document_version,
        "AutomationExecutionStatus": e.status,
        "Mode": e.mode,
        "ExecutionStartTime": e.started_time,
        "ExecutionEndTime": e.end_time,
    })
}

pub fn start_automation_execution(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let document_name = input["DocumentName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DocumentName is required"))?
        .to_string();

    let document_version = input["DocumentVersion"]
        .as_str()
        .unwrap_or("1")
        .to_string();
    let mode = input["Mode"].as_str().unwrap_or("Auto").to_string();
    let parameters = input["Parameters"].clone();

    let execution_id = Uuid::new_v4().to_string();
    let now = now_epoch_secs();

    let execution = SsmAutomationExecution {
        execution_id: execution_id.clone(),
        document_name,
        document_version,
        status: "Success".to_string(),
        mode,
        parameters,
        started_time: now,
        end_time: Some(now),
    };

    state.automation_executions.insert(execution_id.clone(), execution);

    Ok(json!({ "AutomationExecutionId": execution_id }))
}

pub fn get_automation_execution(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let execution_id = input["AutomationExecutionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AutomationExecutionId is required"))?;

    let execution = state.automation_executions.get(execution_id).ok_or_else(|| {
        AwsError::not_found(
            "AutomationExecutionNotFoundException",
            format!("Automation execution '{execution_id}' not found"),
        )
    })?;

    Ok(json!({
        "AutomationExecution": {
            "AutomationExecutionId": execution.execution_id,
            "DocumentName": execution.document_name,
            "DocumentVersion": execution.document_version,
            "AutomationExecutionStatus": execution.status,
            "Mode": execution.mode,
            "Parameters": execution.parameters,
            "Outputs": {},
            "StepExecutions": [],
            "ExecutionStartTime": execution.started_time,
            "ExecutionEndTime": execution.end_time,
        }
    }))
}

pub fn describe_automation_executions(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let executions: Vec<Value> = state
        .automation_executions
        .iter()
        .map(|e| execution_summary(e.value()))
        .take(max_results)
        .collect();

    Ok(json!({ "AutomationExecutionMetadataList": executions }))
}

pub fn stop_automation_execution(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let execution_id = input["AutomationExecutionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AutomationExecutionId is required"))?;

    let mut execution = state.automation_executions.get_mut(execution_id).ok_or_else(|| {
        AwsError::not_found(
            "AutomationExecutionNotFoundException",
            format!("Automation execution '{execution_id}' not found"),
        )
    })?;

    execution.status = "Cancelled".to_string();
    execution.end_time = Some(now_epoch_secs());

    Ok(json!({}))
}
