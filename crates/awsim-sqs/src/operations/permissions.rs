use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;
use crate::util::queue_name_from_url;

/// AddPermission — add a permission to the queue policy document.
/// Stored as a JSON policy under the queue's "QueuePolicy" attribute.
pub fn add_permission(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;
    let label = input["Label"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Label is required"))?;
    let aws_account_ids = input["AWSAccountIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = input["Actions"].as_array().cloned().unwrap_or_default();

    let queue_name = queue_name_from_url(queue_url)?;
    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    // Load existing policy or start fresh
    let mut policy: Value = queue
        .attributes
        .get("Policy")
        .and_then(|p| serde_json::from_str(p).ok())
        .unwrap_or_else(|| {
            json!({
                "Version": "2012-10-17",
                "Statement": []
            })
        });

    // Add new statement
    let statement = json!({
        "Sid": label,
        "Effect": "Allow",
        "Principal": { "AWS": aws_account_ids },
        "Action": actions,
        "Resource": queue.arn
    });

    if let Some(stmts) = policy["Statement"].as_array_mut() {
        stmts.push(statement);
    }

    queue
        .attributes
        .insert("Policy".to_string(), policy.to_string());

    Ok(json!({}))
}

/// RemovePermission — remove a permission statement identified by Label.
pub fn remove_permission(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_url = input["QueueUrl"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "QueueUrl is required"))?;
    let label = input["Label"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Label is required"))?;

    let queue_name = queue_name_from_url(queue_url)?;
    let mut queue = state.queues.get_mut(&queue_name).ok_or_else(|| {
        AwsError::bad_request(
            "AWS.SimpleQueueService.NonExistentQueue",
            format!("The specified queue does not exist: {queue_url}"),
        )
    })?;

    if let Some(raw_policy) = queue.attributes.get("Policy").cloned()
        && let Ok(mut policy) = serde_json::from_str::<Value>(&raw_policy)
    {
        if let Some(stmts) = policy["Statement"].as_array_mut() {
            stmts.retain(|s| s["Sid"].as_str() != Some(label));
        }
        queue
            .attributes
            .insert("Policy".to_string(), policy.to_string());
    }

    Ok(json!({}))
}
