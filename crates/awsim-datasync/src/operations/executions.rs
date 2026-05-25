use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DataSyncState, TaskExecution};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn start_task_execution(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let task_arn = input["TaskArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TaskArn is required"))?
        .to_string();

    if !state.tasks.contains_key(&task_arn) {
        return Err(AwsError::bad_request(
            "InvalidRequestException",
            format!("Task not found: {task_arn}"),
        ));
    }

    let id = uuid::Uuid::new_v4().simple().to_string();
    let exec_arn = format!("{task_arn}/execution/exec-{}", &id[..17]);

    let exec = TaskExecution {
        arn: exec_arn.clone(),
        task_arn,
        status: "SUCCESS".to_string(),
        started_at: now_secs(),
    };
    state.executions.insert(exec_arn.clone(), exec);

    Ok(json!({ "TaskExecutionArn": exec_arn }))
}

pub fn describe_task_execution(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["TaskExecutionArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "TaskExecutionArn is required")
    })?;

    let e = state.executions.get(arn).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidRequestException",
            format!("Execution not found: {arn}"),
        )
    })?;

    Ok(json!({
        "TaskExecutionArn": e.arn,
        "Status": e.status,
        "StartTime": e.started_at,
        "FilesTransferred": 0,
        "BytesTransferred": 0,
    }))
}

pub fn list_task_executions(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let task_filter = input["TaskArn"].as_str();

    let list: Vec<Value> = state
        .executions
        .iter()
        .filter(|e| task_filter.is_none_or(|t| e.value().task_arn == t))
        .map(|e| {
            let ex = e.value();
            json!({ "TaskExecutionArn": ex.arn, "Status": ex.status })
        })
        .collect();

    Ok(json!({ "TaskExecutions": list }))
}

pub fn cancel_task_execution(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["TaskExecutionArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "TaskExecutionArn is required")
    })?;

    if let Some(mut e) = state.executions.get_mut(arn) {
        e.status = "ERROR".to_string();
    }

    Ok(json!({}))
}
