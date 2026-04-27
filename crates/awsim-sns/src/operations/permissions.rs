use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SnsState;

// ---------------------------------------------------------------------------
// AddPermission
// ---------------------------------------------------------------------------

pub fn add_permission(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;
    let label = input["Label"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Label is required"))?;
    let aws_account_ids = input["AWSAccountIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action_names = input["ActionNames"].as_array().cloned().unwrap_or_default();

    let mut topic = state
        .topics
        .get_mut(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    // Load existing policy or start fresh
    let raw_policy = topic.attributes.get("Policy").cloned().unwrap_or_default();
    let mut policy: Value = if raw_policy.is_empty() {
        json!({
            "Version": "2012-10-17",
            "Statement": []
        })
    } else {
        serde_json::from_str(&raw_policy).unwrap_or_else(|_| {
            json!({
                "Version": "2012-10-17",
                "Statement": []
            })
        })
    };

    let statement = json!({
        "Sid": label,
        "Effect": "Allow",
        "Principal": { "AWS": aws_account_ids },
        "Action": action_names,
        "Resource": topic_arn,
    });

    if let Some(stmts) = policy["Statement"].as_array_mut() {
        stmts.push(statement);
    }

    topic
        .attributes
        .insert("Policy".to_string(), policy.to_string());
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RemovePermission
// ---------------------------------------------------------------------------

pub fn remove_permission(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;
    let label = input["Label"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Label is required"))?;

    let mut topic = state
        .topics
        .get_mut(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    if let Some(raw_policy) = topic.attributes.get("Policy").cloned()
        && let Ok(mut policy) = serde_json::from_str::<Value>(&raw_policy)
    {
        if let Some(stmts) = policy["Statement"].as_array_mut() {
            stmts.retain(|s| s["Sid"].as_str() != Some(label));
        }
        topic
            .attributes
            .insert("Policy".to_string(), policy.to_string());
    }

    Ok(json!({}))
}
