use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{DataSyncState, Location};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn make_arn(ctx: &RequestContext) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!(
        "arn:aws:datasync:{}:{}:location/loc-{}",
        ctx.region,
        ctx.account_id,
        &id[..17]
    )
}

pub fn create_location_s3(
    state: &DataSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let bucket = input["S3BucketArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "S3BucketArn is required")
    })?;
    let subdir = input["Subdirectory"].as_str().unwrap_or("/");
    let uri = format!(
        "s3://{}{}",
        bucket.trim_start_matches("arn:aws:s3:::"),
        subdir
    );

    let arn = make_arn(ctx);
    let loc = Location {
        arn: arn.clone(),
        uri,
        location_type: "S3".to_string(),
        config: input.clone(),
        created_at: now_secs(),
    };
    state.locations.insert(arn.clone(), loc);

    Ok(json!({ "LocationArn": arn }))
}

pub fn create_location_nfs(
    state: &DataSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let server = input["ServerHostname"].as_str().unwrap_or("localhost");
    let subdir = input["Subdirectory"].as_str().unwrap_or("/");
    let uri = format!("nfs://{server}{subdir}");

    let arn = make_arn(ctx);
    let loc = Location {
        arn: arn.clone(),
        uri,
        location_type: "NFS".to_string(),
        config: input.clone(),
        created_at: now_secs(),
    };
    state.locations.insert(arn.clone(), loc);

    Ok(json!({ "LocationArn": arn }))
}

pub fn create_location_smb(
    state: &DataSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let server = input["ServerHostname"].as_str().unwrap_or("localhost");
    let subdir = input["Subdirectory"].as_str().unwrap_or("/");
    let uri = format!("smb://{server}{subdir}");

    let arn = make_arn(ctx);
    let loc = Location {
        arn: arn.clone(),
        uri,
        location_type: "SMB".to_string(),
        config: input.clone(),
        created_at: now_secs(),
    };
    state.locations.insert(arn.clone(), loc);

    Ok(json!({ "LocationArn": arn }))
}

pub fn create_location_efs(
    state: &DataSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let fs = input["EfsFilesystemArn"].as_str().unwrap_or("");
    let subdir = input["Subdirectory"].as_str().unwrap_or("/");
    let uri = format!("efs://{fs}{subdir}");

    let arn = make_arn(ctx);
    let loc = Location {
        arn: arn.clone(),
        uri,
        location_type: "EFS".to_string(),
        config: input.clone(),
        created_at: now_secs(),
    };
    state.locations.insert(arn.clone(), loc);

    Ok(json!({ "LocationArn": arn }))
}

pub fn describe_location_s3(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["LocationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "LocationArn is required")
    })?;

    let loc = state.locations.get(arn).ok_or_else(|| {
        AwsError::not_found(
            "InvalidRequestException",
            format!("Location not found: {arn}"),
        )
    })?;

    Ok(json!({
        "LocationArn": loc.arn,
        "LocationUri": loc.uri,
        "S3BucketArn": loc.config["S3BucketArn"],
        "S3Config": loc.config["S3Config"],
        "CreationTime": loc.created_at,
    }))
}

pub fn list_locations(
    state: &DataSyncState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .locations
        .iter()
        .map(|e| {
            let l = e.value();
            json!({ "LocationArn": l.arn, "LocationUri": l.uri })
        })
        .collect();

    Ok(json!({ "Locations": list }))
}

pub fn delete_location(
    state: &DataSyncState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["LocationArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", "LocationArn is required")
    })?;
    state.locations.remove(arn);
    Ok(json!({}))
}
