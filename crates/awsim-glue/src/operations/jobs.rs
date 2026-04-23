use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{GlueState, Job, JobRun};

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
// BatchGetJobs
// ---------------------------------------------------------------------------

pub fn batch_get_jobs(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_names = input["JobNames"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobNames is required"))?;

    let mut jobs: Vec<Value> = Vec::new();
    let mut jobs_not_found: Vec<Value> = Vec::new();

    for name_val in job_names {
        if let Some(name) = name_val.as_str() {
            if let Some(job) = state.jobs.get(name) {
                jobs.push(job_to_value(job.value()));
            } else {
                jobs_not_found.push(json!(name));
            }
        }
    }

    Ok(json!({
        "Jobs": jobs,
        "JobsNotFound": jobs_not_found,
    }))
}

// ---------------------------------------------------------------------------
// StartJobRun
// ---------------------------------------------------------------------------

pub fn start_job_run(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    // Verify job exists
    if !state.jobs.contains_key(job_name) {
        return Err(AwsError::not_found(
            "EntityNotFoundException",
            format!("Job not found: {job_name}"),
        ));
    }

    let run_id = Uuid::new_v4().to_string().replace('-', "")[..16].to_string();
    let arguments = input.get("Arguments").cloned();

    let now = now_str();
    let run = JobRun {
        id: run_id.clone(),
        job_name: job_name.to_string(),
        // Immediately mark SUCCEEDED for the emulator
        status: "SUCCEEDED".to_string(),
        started_on: now.clone(),
        completed_on: Some(now),
        arguments,
    };

    info!(job_name = %job_name, run_id = %run_id, "Started Glue job run (stub: SUCCEEDED)");
    state.job_runs.insert(run_id.clone(), run);

    Ok(json!({ "JobRunId": run_id }))
}

// ---------------------------------------------------------------------------
// GetJobRun
// ---------------------------------------------------------------------------

pub fn get_job_run(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;
    let run_id = input["RunId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "RunId is required"))?;

    let run = state.job_runs.get(run_id).ok_or_else(|| {
        AwsError::not_found(
            "EntityNotFoundException",
            format!("Job run not found: {run_id}"),
        )
    })?;

    if run.job_name != job_name {
        return Err(AwsError::not_found(
            "EntityNotFoundException",
            format!("Job run {run_id} not found for job {job_name}"),
        ));
    }

    Ok(json!({ "JobRun": job_run_to_value(&run) }))
}

// ---------------------------------------------------------------------------
// GetJobRuns
// ---------------------------------------------------------------------------

pub fn get_job_runs(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    let runs: Vec<Value> = state
        .job_runs
        .iter()
        .filter(|e| e.value().job_name == job_name)
        .map(|e| job_run_to_value(e.value()))
        .collect();

    Ok(json!({ "JobRuns": runs }))
}

// ---------------------------------------------------------------------------
// BatchStopJobRun
// ---------------------------------------------------------------------------

pub fn batch_stop_job_run(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobName is required"))?;

    let run_ids = input["JobRunIds"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "JobRunIds is required"))?;

    let mut successful: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for id_val in run_ids {
        if let Some(run_id) = id_val.as_str() {
            if let Some(mut run) = state.job_runs.get_mut(run_id) {
                if run.job_name == job_name {
                    run.status = "STOPPED".to_string();
                    successful.push(json!({
                        "JobName": job_name,
                        "JobRunId": run_id,
                    }));
                } else {
                    errors.push(json!({
                        "JobName": job_name,
                        "JobRunId": run_id,
                        "ErrorDetail": {
                            "ErrorCode": "EntityNotFoundException",
                            "ErrorMessage": format!("Run {run_id} not associated with job {job_name}"),
                        }
                    }));
                }
            } else {
                errors.push(json!({
                    "JobName": job_name,
                    "JobRunId": run_id,
                    "ErrorDetail": {
                        "ErrorCode": "EntityNotFoundException",
                        "ErrorMessage": format!("Job run not found: {run_id}"),
                    }
                }));
            }
        }
    }

    Ok(json!({
        "SuccessfulSubmissions": successful,
        "Errors": errors,
    }))
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

fn job_run_to_value(r: &JobRun) -> Value {
    json!({
        "Id": r.id,
        "JobName": r.job_name,
        "JobRunState": r.status,
        "StartedOn": r.started_on,
        "CompletedOn": r.completed_on,
        "Arguments": r.arguments,
    })
}
