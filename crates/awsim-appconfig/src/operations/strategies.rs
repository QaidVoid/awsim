use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, DeploymentStrategy};

fn new_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..7].to_string()
}

fn strategy_to_value(s: &DeploymentStrategy) -> Value {
    json!({
        "Id": s.id,
        "Name": s.name,
        "DeploymentDurationInMinutes": s.deployment_duration_in_minutes,
        "GrowthFactor": s.growth_factor,
        "FinalBakeTimeInMinutes": s.final_bake_time_in_minutes,
        "GrowthType": s.growth_type,
        "ReplicateTo": s.replicate_to,
        "Description": s.description,
    })
}

pub fn ensure_predefined(state: &AppConfigState) {
    let predefined = [
        ("AppConfig.AllAtOnce", 0u32, 100.0, 0u32),
        ("AppConfig.Linear50PercentEvery30Seconds", 1u32, 50.0, 1u32),
        ("AppConfig.Canary10Percent20Minutes", 20u32, 10.0, 10u32),
    ];
    for (name, dur, growth, bake) in predefined {
        if state
            .deployment_strategies
            .iter()
            .any(|e| e.value().name == name)
        {
            continue;
        }
        let s = DeploymentStrategy {
            id: format!("AppConfig.{}", name.replace("AppConfig.", "")),
            name: name.to_string(),
            deployment_duration_in_minutes: dur,
            growth_factor: growth,
            final_bake_time_in_minutes: bake,
            growth_type: "LINEAR".to_string(),
            replicate_to: "NONE".to_string(),
            description: Some("AWS-managed predefined strategy".to_string()),
        };
        state.deployment_strategies.insert(s.id.clone(), s);
    }
}

pub fn create_strategy(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = new_short_id();
    let s = DeploymentStrategy {
        id: id.clone(),
        name: input
            .get("Name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AwsError::bad_request("BadRequestException", "Name is required"))?
            .to_string(),
        deployment_duration_in_minutes: input
            .get("DeploymentDurationInMinutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32,
        growth_factor: input
            .get("GrowthFactor")
            .and_then(|v| v.as_f64())
            .unwrap_or(20.0),
        final_bake_time_in_minutes: input
            .get("FinalBakeTimeInMinutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32,
        growth_type: input
            .get("GrowthType")
            .and_then(|v| v.as_str())
            .unwrap_or("LINEAR")
            .to_string(),
        replicate_to: input
            .get("ReplicateTo")
            .and_then(|v| v.as_str())
            .unwrap_or("NONE")
            .to_string(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let result = strategy_to_value(&s);
    state.deployment_strategies.insert(id, s);
    Ok(result)
}

pub fn list_strategies(
    state: &AppConfigState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    ensure_predefined(state);
    let items: Vec<Value> = state
        .deployment_strategies
        .iter()
        .map(|e| strategy_to_value(e.value()))
        .collect();
    Ok(json!({ "Items": items }))
}

/// AWS-predefined strategies all share the `AppConfig.` id prefix and
/// are read-only: `Update` and `Delete` against them return
/// `BadRequestException`. We use the prefix check rather than a
/// hardcoded set so seeding can add new ones without re-touching the
/// immutability gate.
fn is_predefined(id: &str) -> bool {
    id.starts_with("AppConfig.")
}

pub fn get_strategy(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    ensure_predefined(state);
    let id = input
        .get("DeploymentStrategyId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "DeploymentStrategyId is required")
        })?;
    let s = state.deployment_strategies.get(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Deployment strategy {id} not found"),
        )
    })?;
    Ok(strategy_to_value(&s))
}

pub fn update_strategy(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("DeploymentStrategyId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "DeploymentStrategyId is required")
        })?;
    if is_predefined(id) {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("Cannot modify the predefined deployment strategy `{id}`."),
        ));
    }
    let mut s = state.deployment_strategies.get_mut(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Deployment strategy {id} not found"),
        )
    })?;
    if let Some(n) = input.get("Description").and_then(|v| v.as_str()) {
        s.description = Some(n.to_string());
    }
    if let Some(n) = input
        .get("DeploymentDurationInMinutes")
        .and_then(|v| v.as_u64())
    {
        s.deployment_duration_in_minutes = n as u32;
    }
    if let Some(n) = input.get("FinalBakeTimeInMinutes").and_then(|v| v.as_u64()) {
        s.final_bake_time_in_minutes = n as u32;
    }
    if let Some(n) = input.get("GrowthFactor").and_then(|v| v.as_f64()) {
        s.growth_factor = n;
    }
    if let Some(n) = input.get("GrowthType").and_then(|v| v.as_str()) {
        s.growth_type = n.to_string();
    }
    Ok(strategy_to_value(&s))
}

pub fn delete_strategy(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("DeploymentStrategyId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "DeploymentStrategyId is required")
        })?;
    if is_predefined(id) {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("Cannot delete the predefined deployment strategy `{id}`."),
        ));
    }
    state.deployment_strategies.remove(id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Deployment strategy {id} not found"),
        )
    })?;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("appconfig", "us-east-1")
    }

    #[test]
    fn predefined_strategies_are_seeded() {
        let state = AppConfigState::default();
        let resp = list_strategies(&state, &json!({}), &ctx()).unwrap();
        let names: Vec<String> = resp["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["Name"].as_str().unwrap().to_string())
            .collect();
        for expected in [
            "AppConfig.AllAtOnce",
            "AppConfig.Linear50PercentEvery30Seconds",
            "AppConfig.Canary10Percent20Minutes",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn predefined_strategy_cannot_be_updated() {
        let state = AppConfigState::default();
        ensure_predefined(&state);
        let err = update_strategy(
            &state,
            &json!({
                "DeploymentStrategyId": "AppConfig.AllAtOnce",
                "Description": "hijacked",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn predefined_strategy_cannot_be_deleted() {
        let state = AppConfigState::default();
        ensure_predefined(&state);
        let err = delete_strategy(
            &state,
            &json!({ "DeploymentStrategyId": "AppConfig.Canary10Percent20Minutes" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn user_strategy_can_be_updated_and_deleted() {
        let state = AppConfigState::default();
        let created = create_strategy(
            &state,
            &json!({
                "Name": "mine",
                "DeploymentDurationInMinutes": 5,
                "GrowthFactor": 25.0,
                "FinalBakeTimeInMinutes": 5,
            }),
            &ctx(),
        )
        .unwrap();
        let id = created["Id"].as_str().unwrap().to_string();
        update_strategy(
            &state,
            &json!({ "DeploymentStrategyId": id, "Description": "ok" }),
            &ctx(),
        )
        .unwrap();
        delete_strategy(&state, &json!({ "DeploymentStrategyId": id }), &ctx()).unwrap();
    }
}
