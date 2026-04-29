use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, Application};

fn new_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..7].to_string()
}

fn app_to_value(a: &Application) -> Value {
    json!({ "Id": a.id, "Name": a.name, "Description": a.description })
}

pub fn create_application(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Name is required"))?
        .to_string();
    let id = new_short_id();
    let a = Application {
        id: id.clone(),
        name,
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let result = app_to_value(&a);
    state.applications.insert(id, a);
    Ok(result)
}

pub fn get_application(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))?;
    let a = state.applications.get(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Application {id} not found"),
        )
    })?;
    Ok(app_to_value(&a))
}

pub fn list_applications(
    state: &AppConfigState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .applications
        .iter()
        .map(|e| app_to_value(e.value()))
        .collect();
    Ok(json!({ "Items": items }))
}

pub fn update_application(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))?;
    let mut a = state.applications.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Application {id} not found"),
        )
    })?;
    if let Some(n) = input.get("Name").and_then(|v| v.as_str()) {
        a.name = n.to_string();
    }
    if let Some(d) = input.get("Description").and_then(|v| v.as_str()) {
        a.description = Some(d.to_string());
    }
    Ok(app_to_value(&a))
}

pub fn delete_application(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))?;
    state.applications.remove(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Application {id} not found"),
        )
    })?;
    let prefix = format!("{id}:");
    state.environments.retain(|k, _| !k.starts_with(&prefix));
    state.profiles.retain(|k, _| !k.starts_with(&prefix));
    state.hosted_versions.retain(|k, _| !k.starts_with(&prefix));
    state.deployments.retain(|k, _| !k.starts_with(&prefix));
    Ok(json!({}))
}
