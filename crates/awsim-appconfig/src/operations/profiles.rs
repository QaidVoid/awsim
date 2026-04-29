use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, ConfigProfile, profile_key};

fn new_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..7].to_string()
}

fn profile_to_value(p: &ConfigProfile) -> Value {
    json!({
        "ApplicationId": p.application_id,
        "Id": p.id,
        "Name": p.name,
        "LocationUri": p.location_uri,
        "RetrievalRoleArn": p.retrieval_role_arn,
        "Type": p.r#type,
        "Validators": p.validators,
        "Description": p.description,
    })
}

fn require_app_id(input: &Value) -> Result<&str, AwsError> {
    input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))
}

pub fn create_profile(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?.to_string();
    if !state.applications.contains_key(&app_id) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Application {app_id} not found"),
        ));
    }
    let id = new_short_id();
    let p = ConfigProfile {
        id: id.clone(),
        application_id: app_id.clone(),
        name: input
            .get("Name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AwsError::bad_request("BadRequestException", "Name is required"))?
            .to_string(),
        location_uri: input
            .get("LocationUri")
            .and_then(|v| v.as_str())
            .unwrap_or("hosted")
            .to_string(),
        retrieval_role_arn: input
            .get("RetrievalRoleArn")
            .and_then(|v| v.as_str())
            .map(String::from),
        r#type: input
            .get("Type")
            .and_then(|v| v.as_str())
            .unwrap_or("AWS.Freeform")
            .to_string(),
        validators: input
            .get("Validators")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        latest_version_number: 0,
    };
    let result = profile_to_value(&p);
    state.profiles.insert(profile_key(&app_id, &id), p);
    Ok(result)
}

pub fn get_profile(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let pid = input
        .get("ConfigurationProfileId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "ConfigurationProfileId is required")
        })?;
    let p = state
        .profiles
        .get(&profile_key(app_id, pid))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Profile {pid} not found"),
            )
        })?;
    Ok(profile_to_value(&p))
}

pub fn list_profiles(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let items: Vec<Value> = state
        .profiles
        .iter()
        .filter(|e| e.value().application_id == app_id)
        .map(|e| profile_to_value(e.value()))
        .collect();
    Ok(json!({ "Items": items }))
}

pub fn delete_profile(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let pid = input
        .get("ConfigurationProfileId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "ConfigurationProfileId is required")
        })?;
    state
        .profiles
        .remove(&profile_key(app_id, pid))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Profile {pid} not found"),
            )
        })?;
    let prefix = format!("{app_id}:{pid}:");
    state.hosted_versions.retain(|k, _| !k.starts_with(&prefix));
    Ok(json!({}))
}
