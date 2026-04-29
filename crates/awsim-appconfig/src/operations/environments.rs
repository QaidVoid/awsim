use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, Environment, env_key};

fn new_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..7].to_string()
}

fn env_to_value(e: &Environment) -> Value {
    json!({
        "ApplicationId": e.application_id,
        "Id": e.id,
        "Name": e.name,
        "Description": e.description,
        "State": e.state,
        "Monitors": e.monitors,
    })
}

fn require_app_id(input: &Value) -> Result<&str, AwsError> {
    input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))
}

pub fn create_environment(
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
    let name = input
        .get("Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Name is required"))?
        .to_string();
    let id = new_short_id();
    let e = Environment {
        id: id.clone(),
        application_id: app_id.clone(),
        name,
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        state: "ReadyForDeployment".to_string(),
        monitors: input
            .get("Monitors")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    };
    let result = env_to_value(&e);
    state.environments.insert(env_key(&app_id, &id), e);
    Ok(result)
}

pub fn get_environment(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let env_id = input
        .get("EnvironmentId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "EnvironmentId is required"))?;
    let e = state
        .environments
        .get(&env_key(app_id, env_id))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Environment {env_id} not found"),
            )
        })?;
    Ok(env_to_value(&e))
}

pub fn list_environments(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let items: Vec<Value> = state
        .environments
        .iter()
        .filter(|e| e.value().application_id == app_id)
        .map(|e| env_to_value(e.value()))
        .collect();
    Ok(json!({ "Items": items }))
}

pub fn delete_environment(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let env_id = input
        .get("EnvironmentId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "EnvironmentId is required"))?;
    state
        .environments
        .remove(&env_key(app_id, env_id))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Environment {env_id} not found"),
            )
        })?;
    Ok(json!({}))
}
