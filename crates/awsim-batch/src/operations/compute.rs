use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BatchState, ComputeEnvironment};

pub fn create_compute_environment(
    state: &BatchState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["computeEnvironmentName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("ValidationException", "computeEnvironmentName is required")
        })?
        .to_string();

    let env_type = input["type"].as_str().unwrap_or("MANAGED").to_string();
    let service_role = input["serviceRole"].as_str().unwrap_or("").to_string();
    let resources = input["computeResources"].clone();

    let arn = format!(
        "arn:aws:batch:{}:{}:compute-environment/{}",
        ctx.region, ctx.account_id, name
    );

    if state.compute_environments.contains_key(&name) {
        return Err(AwsError::conflict(
            "ClientException",
            format!("Compute environment '{name}' already exists"),
        ));
    }

    let env = ComputeEnvironment {
        name: name.clone(),
        arn: arn.clone(),
        env_type,
        state: input["state"].as_str().unwrap_or("ENABLED").to_string(),
        status: "VALID".to_string(),
        compute_resources: resources,
        service_role,
    };

    state.compute_environments.insert(name.clone(), env);

    Ok(json!({
        "computeEnvironmentName": name,
        "computeEnvironmentArn": arn,
    }))
}

pub fn describe_compute_environments(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = input["computeEnvironments"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let list: Vec<Value> = state
        .compute_environments
        .iter()
        .filter(|e| names.is_empty() || names.contains(e.key()) || names.contains(&e.value().arn))
        .map(|e| {
            let env = e.value();
            json!({
                "computeEnvironmentName": env.name,
                "computeEnvironmentArn": env.arn,
                "type": env.env_type,
                "state": env.state,
                "status": env.status,
                "statusReason": "",
                "computeResources": env.compute_resources,
                "serviceRole": env.service_role,
            })
        })
        .collect();

    Ok(json!({ "computeEnvironments": list }))
}

pub fn update_compute_environment(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["computeEnvironment"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "computeEnvironment is required")
    })?;

    let mut env = state.compute_environments.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ClientException",
            format!("Compute environment not found: {name}"),
        )
    })?;

    if let Some(s) = input["state"].as_str() {
        env.state = s.to_string();
    }
    if !input["computeResources"].is_null() {
        env.compute_resources = input["computeResources"].clone();
    }

    Ok(json!({
        "computeEnvironmentName": env.name,
        "computeEnvironmentArn": env.arn,
    }))
}

pub fn delete_compute_environment(
    state: &BatchState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["computeEnvironment"].as_str().ok_or_else(|| {
        AwsError::bad_request("ValidationException", "computeEnvironment is required")
    })?;
    state.compute_environments.remove(name);
    Ok(json!({}))
}
