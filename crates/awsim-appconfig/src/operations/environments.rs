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
    let monitors = input
        .get("Monitors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    validate_monitors(&monitors)?;
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
        monitors,
    };
    let result = env_to_value(&e);
    state.environments.insert(env_key(&app_id, &id), e);
    Ok(result)
}

/// AWS AppConfig environments accept up to 5 `Monitors`; each
/// `AlarmArn` must look like a CloudWatch alarm ARN
/// (`arn:<partition>:cloudwatch:<region>:<account>:alarm:<name>`).
fn validate_monitors(monitors: &[Value]) -> Result<(), AwsError> {
    if monitors.len() > 5 {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!(
                "Environment Monitors has {} entries; the maximum is 5.",
                monitors.len(),
            ),
        ));
    }
    for m in monitors {
        let arn = m.get("AlarmArn").and_then(Value::as_str).ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "Monitor.AlarmArn is required")
        })?;
        // Format: arn:<partition>:cloudwatch:<region>:<account>:alarm:<name>
        let parts: Vec<&str> = arn.splitn(7, ':').collect();
        let shape_ok = parts.len() == 7
            && parts[0] == "arn"
            && !parts[1].is_empty()
            && parts[2] == "cloudwatch"
            && !parts[3].is_empty()
            && !parts[4].is_empty()
            && parts[5] == "alarm"
            && !parts[6].is_empty();
        if !shape_ok {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!("AlarmArn `{arn}` is not a valid CloudWatch alarm ARN."),
            ));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::applications::create_application;

    fn ctx() -> RequestContext {
        RequestContext::new("appconfig", "us-east-1")
    }

    fn alarm_arn(name: &str) -> String {
        format!("arn:aws:cloudwatch:us-east-1:123456789012:alarm:{name}")
    }

    fn setup() -> (AppConfigState, String) {
        let state = AppConfigState::default();
        let app = create_application(&state, &json!({ "Name": "a" }), &ctx()).unwrap();
        (state, app["Id"].as_str().unwrap().to_string())
    }

    #[test]
    fn create_environment_accepts_up_to_five_monitors() {
        let (state, app_id) = setup();
        let monitors: Vec<Value> = (0..5)
            .map(|i| json!({ "AlarmArn": alarm_arn(&format!("a{i}")) }))
            .collect();
        create_environment(
            &state,
            &json!({ "ApplicationId": app_id, "Name": "env", "Monitors": monitors }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_environment_rejects_six_monitors() {
        let (state, app_id) = setup();
        let monitors: Vec<Value> = (0..6)
            .map(|i| json!({ "AlarmArn": alarm_arn(&format!("a{i}")) }))
            .collect();
        let err = create_environment(
            &state,
            &json!({ "ApplicationId": app_id, "Name": "env", "Monitors": monitors }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
        assert!(err.message.contains("maximum is 5"));
    }

    #[test]
    fn create_environment_rejects_bad_alarm_arn() {
        let (state, app_id) = setup();
        for bad in [
            "not-an-arn",
            "arn:aws:s3:::my-bucket",
            "arn:aws:cloudwatch:us-east-1:123456789012:metricalarm:name",
            "arn:aws:cloudwatch::123456789012:alarm:name",
        ] {
            let err = create_environment(
                &state,
                &json!({
                    "ApplicationId": app_id,
                    "Name": "env",
                    "Monitors": [{ "AlarmArn": bad }],
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "BadRequestException", "input {bad}");
        }
    }
}
