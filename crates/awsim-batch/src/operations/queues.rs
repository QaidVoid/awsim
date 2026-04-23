use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BatchState, JobQueue};

pub fn create_job_queue(
    state: &BatchState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["jobQueueName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobQueueName is required"))?
        .to_string();

    let priority = input["priority"].as_u64().unwrap_or(1);
    let order = input["computeEnvironmentOrder"].clone();

    let arn = format!(
        "arn:aws:batch:{}:{}:job-queue/{}",
        ctx.region, ctx.account_id, name
    );

    if state.job_queues.contains_key(&name) {
        return Err(AwsError::conflict(
            "ClientException",
            format!("Job queue '{name}' already exists"),
        ));
    }

    let queue = JobQueue {
        name: name.clone(),
        arn: arn.clone(),
        state: input["state"].as_str().unwrap_or("ENABLED").to_string(),
        status: "VALID".to_string(),
        priority,
        compute_environment_order: order,
    };

    state.job_queues.insert(name.clone(), queue);

    Ok(json!({
        "jobQueueName": name,
        "jobQueueArn": arn,
    }))
}

pub fn describe_job_queues(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = input["jobQueues"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let list: Vec<Value> = state
        .job_queues
        .iter()
        .filter(|e| names.is_empty() || names.contains(e.key()) || names.contains(&e.value().arn))
        .map(|e| {
            let q = e.value();
            json!({
                "jobQueueName": q.name,
                "jobQueueArn": q.arn,
                "state": q.state,
                "status": q.status,
                "statusReason": "",
                "priority": q.priority,
                "computeEnvironmentOrder": q.compute_environment_order,
            })
        })
        .collect();

    Ok(json!({ "jobQueues": list }))
}

pub fn update_job_queue(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["jobQueue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobQueue is required"))?;

    let mut q = state.job_queues.get_mut(name).ok_or_else(|| {
        AwsError::not_found("ClientException", format!("Job queue not found: {name}"))
    })?;

    if let Some(s) = input["state"].as_str() {
        q.state = s.to_string();
    }
    if let Some(p) = input["priority"].as_u64() {
        q.priority = p;
    }
    if !input["computeEnvironmentOrder"].is_null() {
        q.compute_environment_order = input["computeEnvironmentOrder"].clone();
    }

    Ok(json!({
        "jobQueueName": q.name,
        "jobQueueArn": q.arn,
    }))
}

pub fn delete_job_queue(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["jobQueue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobQueue is required"))?;
    state.job_queues.remove(name);
    Ok(json!({}))
}
