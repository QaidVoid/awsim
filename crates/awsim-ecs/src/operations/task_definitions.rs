use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::clusters::now_epoch_str;
use crate::state::{EcsState, TaskDefinition};

fn task_def_to_json(td: &TaskDefinition) -> Value {
    json!({
        "taskDefinitionArn": td.arn,
        "family": td.family,
        "revision": td.revision,
        "status": td.status,
        "containerDefinitions": td.container_definitions,
        "networkMode": td.network_mode,
        "requiresCompatibilities": td.requires_compatibilities,
        "registeredAt": now_epoch_str(),
    })
}

/// Parse "family:revision" or just "family" or an ARN into (family, optional revision).
pub fn parse_task_definition_id(id: &str) -> (&str, Option<u32>) {
    // ARN: arn:aws:ecs:{region}:{account}:task-definition/{family}:{revision}
    let base = if id.starts_with("arn:") {
        id.split('/').last().unwrap_or(id)
    } else {
        id
    };
    if let Some(colon_pos) = base.rfind(':') {
        let family = &base[..colon_pos];
        if let Ok(rev) = base[colon_pos + 1..].parse::<u32>() {
            return (family, Some(rev));
        }
    }
    (base, None)
}

// ---------------------------------------------------------------------------
// RegisterTaskDefinition
// ---------------------------------------------------------------------------

pub fn register_task_definition(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let family = input["family"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "family is required"))?
        .to_string();

    let container_definitions = input["containerDefinitions"].clone();
    let network_mode = input["networkMode"].as_str().unwrap_or("bridge").to_string();
    let requires_compatibilities: Vec<String> = input["requiresCompatibilities"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let revision = {
        let mut revisions = state.task_definitions.entry(family.clone()).or_default();
        let rev = revisions.len() as u32 + 1;
        let arn = format!(
            "arn:aws:ecs:{}:{}:task-definition/{}:{}",
            ctx.region, ctx.account_id, family, rev
        );
        let td = TaskDefinition {
            family: family.clone(),
            revision: rev,
            arn,
            container_definitions,
            status: "ACTIVE".to_string(),
            network_mode,
            requires_compatibilities,
        };
        revisions.push(td);
        rev
    };

    let td_json = {
        let revisions = state.task_definitions.get(&family).unwrap();
        task_def_to_json(&revisions[(revision - 1) as usize])
    };

    info!(family = %family, revision = revision, "Registered ECS task definition");

    Ok(json!({ "taskDefinition": td_json }))
}

// ---------------------------------------------------------------------------
// DeregisterTaskDefinition
// ---------------------------------------------------------------------------

pub fn deregister_task_definition(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let td_id = input["taskDefinition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "taskDefinition is required"))?;

    let (family, maybe_rev) = parse_task_definition_id(td_id);

    let mut revisions = state.task_definitions.get_mut(family).ok_or_else(|| {
        AwsError::not_found(
            "ClientException",
            format!("The specified task definition does not exist: {td_id}"),
        )
    })?;

    let rev = maybe_rev.ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Revision must be specified when deregistering")
    })?;

    let idx = (rev - 1) as usize;
    if idx >= revisions.len() {
        return Err(AwsError::not_found(
            "ClientException",
            format!("The specified task definition does not exist: {td_id}"),
        ));
    }

    revisions[idx].status = "INACTIVE".to_string();
    let td_json = task_def_to_json(&revisions[idx]);

    info!(family = %family, revision = rev, "Deregistered ECS task definition");

    Ok(json!({ "taskDefinition": td_json }))
}

// ---------------------------------------------------------------------------
// DescribeTaskDefinition
// ---------------------------------------------------------------------------

pub fn describe_task_definition(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let td_id = input["taskDefinition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "taskDefinition is required"))?;

    let (family, maybe_rev) = parse_task_definition_id(td_id);

    let revisions = state.task_definitions.get(family).ok_or_else(|| {
        AwsError::not_found(
            "ClientException",
            format!("The specified task definition does not exist: {td_id}"),
        )
    })?;

    let td = if let Some(rev) = maybe_rev {
        let idx = (rev - 1) as usize;
        revisions.get(idx).ok_or_else(|| {
            AwsError::not_found(
                "ClientException",
                format!("The specified task definition does not exist: {td_id}"),
            )
        })?
    } else {
        // Latest active
        revisions
            .iter()
            .rev()
            .find(|td| td.status == "ACTIVE")
            .ok_or_else(|| {
                AwsError::not_found(
                    "ClientException",
                    format!("No active task definition found for family: {family}"),
                )
            })?
    };

    Ok(json!({ "taskDefinition": task_def_to_json(td) }))
}

// ---------------------------------------------------------------------------
// ListTaskDefinitions
// ---------------------------------------------------------------------------

pub fn list_task_definitions(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let family_prefix = input["familyPrefix"].as_str().unwrap_or("");

    let arns: Vec<Value> = state
        .task_definitions
        .iter()
        .filter(|entry| entry.key().starts_with(family_prefix))
        .flat_map(|entry| {
            entry
                .value()
                .iter()
                .filter(|td| td.status == "ACTIVE")
                .map(|td| json!(td.arn))
                .collect::<Vec<_>>()
        })
        .collect();

    Ok(json!({ "taskDefinitionArns": arns }))
}

// ---------------------------------------------------------------------------
// ListTaskDefinitionFamilies
// ---------------------------------------------------------------------------

pub fn list_task_definition_families(
    state: &EcsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let families: Vec<Value> = state
        .task_definitions
        .iter()
        .map(|entry| json!(entry.key()))
        .collect();

    Ok(json!({ "families": families }))
}
