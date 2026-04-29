use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, Deployment, deployment_key, env_key};

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

fn deployment_to_value(d: &Deployment) -> Value {
    json!({
        "ApplicationId": d.application_id,
        "EnvironmentId": d.environment_id,
        "DeploymentNumber": d.deployment_number,
        "ConfigurationProfileId": d.configuration_profile_id,
        "DeploymentStrategyId": d.deployment_strategy_id,
        "ConfigurationVersion": d.configuration_version,
        "State": d.state,
        "PercentageComplete": d.percentage_complete,
        "EventLog": d.event_log,
    })
}

pub fn start_deployment(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_str(input, "ApplicationId")?.to_string();
    let env_id = require_str(input, "EnvironmentId")?.to_string();
    if !state.environments.contains_key(&env_key(&app_id, &env_id)) {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Environment {env_id} not found"),
        ));
    }
    let next_num = state
        .deployments
        .iter()
        .filter(|e| {
            let d = e.value();
            d.application_id == app_id && d.environment_id == env_id
        })
        .map(|e| e.value().deployment_number)
        .max()
        .unwrap_or(0)
        + 1;

    let d = Deployment {
        application_id: app_id.clone(),
        environment_id: env_id.clone(),
        deployment_number: next_num,
        configuration_profile_id: require_str(input, "ConfigurationProfileId")?.to_string(),
        deployment_strategy_id: require_str(input, "DeploymentStrategyId")?.to_string(),
        configuration_version: require_str(input, "ConfigurationVersion")?.to_string(),
        // Emulator collapses the deployment lifecycle.
        state: "COMPLETE".to_string(),
        percentage_complete: 100.0,
        event_log: vec![json!({
            "EventType": "DEPLOYMENT_COMPLETED",
            "TriggeredBy": "USER",
            "Description": "Emulator: deployment completed immediately",
        })],
    };
    let result = deployment_to_value(&d);
    state
        .deployments
        .insert(deployment_key(&app_id, &env_id, next_num), d);
    Ok(result)
}

pub fn get_deployment(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_str(input, "ApplicationId")?;
    let env_id = require_str(input, "EnvironmentId")?;
    let num = input
        .get("DeploymentNumber")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "DeploymentNumber is required")
        })? as u32;
    let d = state
        .deployments
        .get(&deployment_key(app_id, env_id, num))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Deployment {num} not found"),
            )
        })?;
    Ok(deployment_to_value(&d))
}

pub fn list_deployments(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_str(input, "ApplicationId")?;
    let env_id = require_str(input, "EnvironmentId")?;
    let items: Vec<Value> = state
        .deployments
        .iter()
        .filter(|e| {
            let d = e.value();
            d.application_id == app_id && d.environment_id == env_id
        })
        .map(|e| deployment_to_value(e.value()))
        .collect();
    Ok(json!({ "Items": items }))
}
