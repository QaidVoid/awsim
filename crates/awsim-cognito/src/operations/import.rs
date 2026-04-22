use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, UserImportJob};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn job_to_value(j: &UserImportJob) -> Value {
    json!({
        "JobId": j.job_id,
        "UserPoolId": j.user_pool_id,
        "JobName": j.job_name,
        "Status": j.status,
        "CloudWatchLogsRoleArn": j.cloud_watch_logs_role_arn,
        "PreSignedUrl": j.pre_signed_url,
        "CreationDate": j.creation_date,
        "StartDate": j.start_date,
        "CompletionDate": j.completion_date,
        "ImportedUsers": j.imported_users,
        "SkippedUsers": j.skipped_users,
        "FailedUsers": j.failed_users
    })
}

/// Standard Cognito CSV header fields.
static CSV_HEADER: &[&str] = &[
    "cognito:username",
    "name",
    "given_name",
    "family_name",
    "middle_name",
    "nickname",
    "preferred_username",
    "profile",
    "picture",
    "website",
    "email",
    "email_verified",
    "gender",
    "birthdate",
    "zoneinfo",
    "locale",
    "phone_number",
    "phone_number_verified",
    "address",
    "updated_at",
    "cognito:mfa_enabled",
    "cognito:phone_number_verified",
];

// ---------------------------------------------------------------------------
// CreateUserImportJob
// ---------------------------------------------------------------------------

pub fn create_user_import_job(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let job_name = input["JobName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "JobName is required"))?;
    let cloud_watch_logs_role_arn = input["CloudWatchLogsRoleArn"].as_str().map(String::from);

    let now = now_epoch();
    let job_id = format!("import-job-{}", Uuid::new_v4());

    let job = UserImportJob {
        job_id: job_id.clone(),
        user_pool_id: pool_id.to_string(),
        job_name: job_name.to_string(),
        status: "Created".to_string(),
        cloud_watch_logs_role_arn,
        pre_signed_url: Some(format!(
            "https://cognito-identity.s3.amazonaws.com/import/{pool_id}/{job_id}.csv"
        )),
        creation_date: now,
        start_date: None,
        completion_date: None,
        imported_users: 0,
        skipped_users: 0,
        failed_users: 0,
    };

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let val = job_to_value(&job);
    pool.import_jobs.push(job);

    info!(pool_id = %pool_id, job_id = %job_id, "Cognito: created user import job");
    Ok(json!({ "UserImportJob": val }))
}

// ---------------------------------------------------------------------------
// DescribeUserImportJob
// ---------------------------------------------------------------------------

pub fn describe_user_import_job(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let job_id = input["JobId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "JobId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let job = pool.import_jobs.iter().find(|j| j.job_id == job_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Import job not found: {job_id}")))?;

    Ok(json!({ "UserImportJob": job_to_value(job) }))
}

// ---------------------------------------------------------------------------
// StartUserImportJob
// ---------------------------------------------------------------------------

pub fn start_user_import_job(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let job_id = input["JobId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "JobId is required"))?;

    let now = now_epoch();
    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let job = pool.import_jobs.iter_mut().find(|j| j.job_id == job_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Import job not found: {job_id}")))?;

    if job.status != "Created" && job.status != "Stopped" {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Job cannot be started in status: {}", job.status),
        ));
    }

    job.status = "Succeeded".to_string(); // Stub: immediately succeed
    job.start_date = Some(now);
    job.completion_date = Some(now);

    let val = job_to_value(job);
    info!(pool_id = %pool_id, job_id = %job_id, "Cognito: started user import job");
    Ok(json!({ "UserImportJob": val }))
}

// ---------------------------------------------------------------------------
// StopUserImportJob
// ---------------------------------------------------------------------------

pub fn stop_user_import_job(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let job_id = input["JobId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "JobId is required"))?;

    let now = now_epoch();
    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let job = pool.import_jobs.iter_mut().find(|j| j.job_id == job_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Import job not found: {job_id}")))?;

    job.status = "Stopped".to_string();
    job.completion_date = Some(now);

    let val = job_to_value(job);
    info!(pool_id = %pool_id, job_id = %job_id, "Cognito: stopped user import job");
    Ok(json!({ "UserImportJob": val }))
}

// ---------------------------------------------------------------------------
// ListUserImportJobs
// ---------------------------------------------------------------------------

pub fn list_user_import_jobs(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let jobs: Vec<Value> = pool.import_jobs.iter().take(max_results).map(job_to_value).collect();
    Ok(json!({ "UserImportJobs": jobs }))
}

// ---------------------------------------------------------------------------
// GetCSVHeader
// ---------------------------------------------------------------------------

pub fn get_csv_header(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    // Verify pool exists
    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let mut headers: Vec<String> = CSV_HEADER.iter().map(|s| s.to_string()).collect();

    // Add any custom attributes defined in the schema
    for schema_attr in &pool.schema {
        let custom_key = format!("custom:{}", schema_attr.name);
        if !headers.contains(&custom_key) {
            headers.push(custom_key);
        }
    }

    Ok(json!({
        "UserPoolId": pool_id,
        "CSVHeader": headers
    }))
}
