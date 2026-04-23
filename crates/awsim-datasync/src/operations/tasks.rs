use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DataSyncState, Task};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn create_task(
    state: &DataSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source = input["SourceLocationArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "SourceLocationArn is required"))?
        .to_string();
    let destination = input["DestinationLocationArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "DestinationLocationArn is required"))?
        .to_string();

    let id = uuid::Uuid::new_v4().simple().to_string();
    let arn = format!(
        "arn:aws:datasync:{}:{}:task/task-{}",
        ctx.region, ctx.account_id, &id[..17]
    );

    let task = Task {
        arn: arn.clone(),
        name: input["Name"].as_str().unwrap_or("").to_string(),
        status: "AVAILABLE".to_string(),
        source_location_arn: source,
        destination_location_arn: destination,
        options: input["Options"].clone(),
        created_at: now_secs(),
    };

    state.tasks.insert(arn.clone(), task);

    Ok(json!({ "TaskArn": arn }))
}

pub fn describe_task(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["TaskArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TaskArn is required"))?;

    let t = state.tasks.get(arn).ok_or_else(|| {
        AwsError::not_found("InvalidRequestException", format!("Task not found: {arn}"))
    })?;

    Ok(json!({
        "TaskArn": t.arn,
        "Status": t.status,
        "Name": t.name,
        "SourceLocationArn": t.source_location_arn,
        "DestinationLocationArn": t.destination_location_arn,
        "Options": t.options,
        "CreationTime": t.created_at,
    }))
}

pub fn list_tasks(
    state: &DataSyncState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .tasks
        .iter()
        .map(|e| {
            let t = e.value();
            json!({ "TaskArn": t.arn, "Status": t.status, "Name": t.name })
        })
        .collect();

    Ok(json!({ "Tasks": list }))
}

pub fn update_task(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["TaskArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TaskArn is required"))?;

    let mut t = state.tasks.get_mut(arn).ok_or_else(|| {
        AwsError::not_found("InvalidRequestException", format!("Task not found: {arn}"))
    })?;

    if let Some(n) = input["Name"].as_str() {
        t.name = n.to_string();
    }
    if !input["Options"].is_null() {
        t.options = input["Options"].clone();
    }

    Ok(json!({}))
}

pub fn delete_task(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["TaskArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidRequestException", "TaskArn is required"))?;
    state.tasks.remove(arn);
    Ok(json!({}))
}
