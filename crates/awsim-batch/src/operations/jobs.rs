use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BatchState, Job, JobDefinition};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn register_job_definition(
    state: &BatchState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["jobDefinitionName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "jobDefinitionName is required")
        })?
        .to_string();

    let job_type = input["type"].as_str().unwrap_or("container").to_string();
    let container_props = input["containerProperties"].clone();

    let revision = {
        let mut r = state
            .job_definition_revisions
            .entry(name.clone())
            .or_insert(0);
        *r += 1;
        *r
    };

    let arn = format!(
        "arn:aws:batch:{}:{}:job-definition/{}:{}",
        ctx.region, ctx.account_id, name, revision
    );

    let def = JobDefinition {
        name: name.clone(),
        arn: arn.clone(),
        revision,
        job_type,
        container_properties: container_props,
        status: "ACTIVE".to_string(),
    };

    let key = format!("{name}:{revision}");
    state.job_definitions.insert(key, def);

    Ok(json!({
        "jobDefinitionName": name,
        "jobDefinitionArn": arn,
        "revision": revision,
    }))
}

pub fn describe_job_definitions(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input["jobDefinitionName"].as_str();

    let list: Vec<Value> = state
        .job_definitions
        .iter()
        .filter(|e| name_filter.is_none_or(|n| e.value().name == n))
        .map(|e| {
            let d = e.value();
            json!({
                "jobDefinitionName": d.name,
                "jobDefinitionArn": d.arn,
                "revision": d.revision,
                "type": d.job_type,
                "status": d.status,
                "containerProperties": d.container_properties,
            })
        })
        .collect();

    Ok(json!({ "jobDefinitions": list }))
}

pub fn deregister_job_definition(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn_or_name = input["jobDefinition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobDefinition is required"))?;

    let keys_to_remove: Vec<String> = state
        .job_definitions
        .iter()
        .filter(|e| e.value().arn == arn_or_name || e.key() == arn_or_name)
        .map(|e| e.key().clone())
        .collect();

    for key in keys_to_remove {
        if let Some((_, mut def)) = state.job_definitions.remove(&key) {
            def.status = "INACTIVE".to_string();
            state.job_definitions.insert(key, def);
        }
    }

    Ok(json!({}))
}

pub fn submit_job(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["jobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobName is required"))?
        .to_string();
    let queue = input["jobQueue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobQueue is required"))?
        .to_string();
    let definition = input["jobDefinition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobDefinition is required"))?
        .to_string();

    let id = uuid::Uuid::new_v4().to_string();
    let job = Job {
        id: id.clone(),
        name: name.clone(),
        queue,
        definition,
        status: "SUBMITTED".to_string(),
        created_at: now_secs(),
    };

    state.jobs.insert(id.clone(), job);

    Ok(json!({
        "jobName": name,
        "jobId": id,
    }))
}

pub fn describe_jobs(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ids: Vec<String> = input["jobs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let list: Vec<Value> = state
        .jobs
        .iter()
        .filter(|e| ids.is_empty() || ids.contains(e.key()))
        .map(|e| {
            let j = e.value();
            json!({
                "jobId": j.id,
                "jobName": j.name,
                "jobQueue": j.queue,
                "jobDefinition": j.definition,
                "status": j.status,
                "createdAt": j.created_at * 1000,
                "startedAt": j.created_at * 1000,
            })
        })
        .collect();

    Ok(json!({ "jobs": list }))
}

pub fn list_jobs(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let queue_filter = input["jobQueue"].as_str();

    let list: Vec<Value> = state
        .jobs
        .iter()
        .filter(|e| {
            queue_filter.is_none_or(|q| e.value().queue == q || e.value().queue.ends_with(q))
        })
        .map(|e| {
            let j = e.value();
            json!({
                "jobId": j.id,
                "jobName": j.name,
                "status": j.status,
                "createdAt": j.created_at * 1000,
            })
        })
        .collect();

    Ok(json!({ "jobSummaryList": list }))
}

pub fn terminate_job(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["jobId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobId is required"))?;

    if let Some(mut j) = state.jobs.get_mut(id) {
        j.status = "FAILED".to_string();
    }

    Ok(json!({}))
}

pub fn cancel_job(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["jobId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("ValidationException", "jobId is required"))?;

    if let Some(mut j) = state.jobs.get_mut(id) {
        j.status = "FAILED".to_string();
    }

    Ok(json!({}))
}
