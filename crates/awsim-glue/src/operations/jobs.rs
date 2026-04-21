use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{GlueState, Job};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// CreateJob
// ---------------------------------------------------------------------------

pub fn create_job(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;
    let role = input["Role"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Role is required"))?;

    if state.jobs.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Job already exists: {name}"),
        ));
    }

    let command = input.get("Command").cloned();
    let default_arguments = input.get("DefaultArguments").cloned();

    let job = Job {
        name: name.to_string(),
        role: role.to_string(),
        command,
        default_arguments,
        created_at: now_str(),
    };

    info!(name = %name, "Created Glue job");
    state.jobs.insert(name.to_string(), job);

    Ok(json!({ "Name": name }))
}

// ---------------------------------------------------------------------------
// GetJob
// ---------------------------------------------------------------------------

pub fn get_job(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    let job = state.jobs.get(job_name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Job not found: {job_name}"))
    })?;

    Ok(json!({ "Job": job_to_value(&job) }))
}

// ---------------------------------------------------------------------------
// GetJobs
// ---------------------------------------------------------------------------

pub fn get_jobs(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .jobs
        .iter()
        .map(|e| job_to_value(e.value()))
        .collect();

    Ok(json!({ "Jobs": list }))
}

// ---------------------------------------------------------------------------
// DeleteJob
// ---------------------------------------------------------------------------

pub fn delete_job(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    state.jobs.remove(job_name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Job not found: {job_name}"))
    })?;

    info!(name = %job_name, "Deleted Glue job");
    Ok(json!({ "JobName": job_name }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn job_to_value(j: &Job) -> Value {
    json!({
        "Name": j.name,
        "Role": j.role,
        "Command": j.command,
        "DefaultArguments": j.default_arguments,
        "CreatedOn": j.created_at,
    })
}
