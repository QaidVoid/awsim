use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{MessageMoveTask, SqsState};

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// StartMessageMoveTask — begin a DLQ redrive task (stub).
pub fn start_message_move_task(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_arn = input["SourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "SourceArn is required"))?;
    let destination_arn = input["DestinationArn"].as_str().map(str::to_string);

    let task_handle = Uuid::new_v4().to_string();

    let task = MessageMoveTask {
        task_handle: task_handle.clone(),
        source_arn: source_arn.to_string(),
        destination_arn,
        status: "RUNNING".to_string(),
        started_timestamp: now_secs(),
        approximate_number_of_messages_moved: 0,
        approximate_number_of_messages_to_move: 0,
    };

    state.move_tasks.insert(task_handle.clone(), task);

    Ok(json!({ "TaskHandle": task_handle }))
}

/// CancelMessageMoveTask — cancel a running DLQ redrive task (stub).
pub fn cancel_message_move_task(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let task_handle = input["TaskHandle"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TaskHandle is required"))?;

    let mut task = state.move_tasks.get_mut(task_handle).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Task not found: {task_handle}"),
        )
    })?;

    if task.status == "COMPLETED" || task.status == "CANCELLED" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Task {} is already in terminal state {}", task_handle, task.status),
        ));
    }

    let moved = task.approximate_number_of_messages_moved;
    task.status = "CANCELLED".to_string();

    Ok(json!({ "ApproximateNumberOfMessagesMoved": moved }))
}

/// ListMessageMoveTasks — list move tasks for a source ARN.
pub fn list_message_move_tasks(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_arn = input["SourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "SourceArn is required"))?;

    let results: Vec<Value> = state
        .move_tasks
        .iter()
        .filter(|entry| entry.value().source_arn == source_arn)
        .map(|entry| {
            let t = entry.value();
            let mut obj = json!({
                "TaskHandle": t.task_handle,
                "SourceArn": t.source_arn,
                "Status": t.status,
                "StartedTimestamp": t.started_timestamp,
                "ApproximateNumberOfMessagesMoved": t.approximate_number_of_messages_moved,
                "ApproximateNumberOfMessagesToMove": t.approximate_number_of_messages_to_move,
            });
            if let Some(dst) = &t.destination_arn {
                obj["DestinationArn"] = Value::String(dst.clone());
            }
            obj
        })
        .collect();

    Ok(json!({ "Results": results }))
}
