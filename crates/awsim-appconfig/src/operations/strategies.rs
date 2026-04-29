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
