use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::clusters::now_epoch_str;
use crate::state::{EcsState, TaskDefinition};

fn task_def_to_json(td: &TaskDefinition) -> Value {
    let mut obj = json!({
        "taskDefinitionArn": td.arn,
        "family": td.family,
        "revision": td.revision,
        "status": td.status,
        "containerDefinitions": td.container_definitions,
        "networkMode": td.network_mode,
        "requiresCompatibilities": td.requires_compatibilities,
        "registeredAt": now_epoch_str(),
        "placementConstraints": td.placement_constraints,
        "placementStrategy": td.placement_strategy,
    });
    if let Some(ref cpu) = td.cpu {
        obj["cpu"] = json!(cpu);
    }
    if let Some(ref mem) = td.memory {
        obj["memory"] = json!(mem);
    }
    obj
}

/// AWS Fargate cpu/memory pair allowlist. Each cpu value maps to the
/// memory values that ECS accepts; any other combination is rejected at
/// RegisterTaskDefinition with ClientException. See:
/// https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task-cpu-memory.html
fn fargate_memory_options(cpu_mib: u32) -> Option<Vec<u32>> {
    match cpu_mib {
        256 => Some(vec![512, 1024, 2048]),
        512 => Some((1024..=4096).step_by(1024).collect()),
        1024 => Some((2048..=8192).step_by(1024).collect()),
        2048 => Some((4096..=16384).step_by(1024).collect()),
        4096 => Some((8192..=30720).step_by(1024).collect()),
        8192 => Some((16384..=61440).step_by(4096).collect()),
        16384 => Some((32768..=122880).step_by(8192).collect()),
        _ => None,
    }
}

fn validate_fargate_cpu_memory(cpu: &str, memory: &str) -> Result<(), AwsError> {
    let cpu_n: u32 = cpu.parse().map_err(|_| {
        AwsError::bad_request(
            "ClientException",
            format!("Task cpu '{cpu}' is not a valid number."),
        )
    })?;
    let mem_n: u32 = memory.parse().map_err(|_| {
        AwsError::bad_request(
            "ClientException",
            format!("Task memory '{memory}' is not a valid number."),
        )
    })?;
    let options = fargate_memory_options(cpu_n).ok_or_else(|| {
        AwsError::bad_request(
            "ClientException",
            format!(
                "Task cpu '{cpu}' is not a valid Fargate vCPU value; \
                 must be one of: 256, 512, 1024, 2048, 4096, 8192, 16384."
            ),
        )
    })?;
    if !options.contains(&mem_n) {
        return Err(AwsError::bad_request(
            "ClientException",
            format!(
                "Task memory '{memory}' MiB is not valid for Fargate cpu '{cpu}'; \
                 allowed values: {}.",
                options
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ));
    }
    Ok(())
}

/// Parse "family:revision" or just "family" or an ARN into (family, optional revision).
pub fn parse_task_definition_id(id: &str) -> (&str, Option<u32>) {
    // ARN: arn:aws:ecs:{region}:{account}:task-definition/{family}:{revision}
    let base = if id.starts_with("arn:") {
        id.split('/').next_back().unwrap_or(id)
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
    let network_mode = input["networkMode"]
        .as_str()
        .unwrap_or("bridge")
        .to_string();
    if !matches!(network_mode.as_str(), "bridge" | "host" | "awsvpc" | "none") {
        return Err(AwsError::bad_request(
            "ClientException",
            format!(
                "networkMode '{network_mode}' is not supported. Must be one of: bridge, host, awsvpc, none."
            ),
        ));
    }
    let requires_compatibilities: Vec<String> = input["requiresCompatibilities"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for compat in &requires_compatibilities {
        if !matches!(compat.as_str(), "EC2" | "FARGATE" | "EXTERNAL") {
            return Err(AwsError::bad_request(
                "ClientException",
                format!(
                    "requiresCompatibilities entry '{compat}' is invalid. Must be one of: EC2, FARGATE, EXTERNAL."
                ),
            ));
        }
    }
    // Fargate tasks must use awsvpc networking; the real ECS API
    // returns ClientException when this combination is wrong.
    let needs_fargate = requires_compatibilities.iter().any(|c| c == "FARGATE");
    if needs_fargate && network_mode != "awsvpc" {
        return Err(AwsError::bad_request(
            "ClientException",
            "Tasks using the Fargate launch type must use the awsvpc network mode.",
        ));
    }

    // placementConstraints + placementStrategy. AWS rejects unknown
    // `type` values at RegisterTaskDefinition with ClientException, so
    // do the same here.
    let placement_constraints: Vec<Value> = input["placementConstraints"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for c in &placement_constraints {
        let t = c.get("type").and_then(Value::as_str).unwrap_or("");
        if !matches!(t, "memberOf" | "distinctInstance") {
            return Err(AwsError::bad_request(
                "ClientException",
                format!("placementConstraints.type `{t}` must be memberOf or distinctInstance."),
            ));
        }
    }
    let placement_strategy: Vec<Value> = input["placementStrategy"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for s in &placement_strategy {
        let t = s.get("type").and_then(Value::as_str).unwrap_or("");
        if !matches!(t, "random" | "spread" | "binpack") {
            return Err(AwsError::bad_request(
                "ClientException",
                format!("placementStrategy.type `{t}` must be random, spread, or binpack."),
            ));
        }
    }

    let cpu = input["cpu"].as_str().map(str::to_string);
    let memory = input["memory"].as_str().map(str::to_string);
    if needs_fargate {
        let cpu_str = cpu.as_deref().ok_or_else(|| {
            AwsError::bad_request("ClientException", "Fargate tasks require task-level cpu.")
        })?;
        let mem_str = memory.as_deref().ok_or_else(|| {
            AwsError::bad_request(
                "ClientException",
                "Fargate tasks require task-level memory.",
            )
        })?;
        validate_fargate_cpu_memory(cpu_str, mem_str)?;
    }

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
            cpu: cpu.clone(),
            memory: memory.clone(),
            placement_constraints: placement_constraints.clone(),
            placement_strategy: placement_strategy.clone(),
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
    let td_id = input["taskDefinition"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "taskDefinition is required")
    })?;

    let (family, maybe_rev) = parse_task_definition_id(td_id);

    let mut revisions = state.task_definitions.get_mut(family).ok_or_else(|| {
        AwsError::bad_request(
            "ClientException",
            format!("The specified task definition does not exist: {td_id}"),
        )
    })?;

    let rev = maybe_rev.ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "Revision must be specified when deregistering",
        )
    })?;

    let idx = (rev - 1) as usize;
    if idx >= revisions.len() {
        return Err(AwsError::bad_request(
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
    let td_id = input["taskDefinition"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "taskDefinition is required")
    })?;

    let (family, maybe_rev) = parse_task_definition_id(td_id);

    let revisions = state.task_definitions.get(family).ok_or_else(|| {
        AwsError::bad_request(
            "ClientException",
            format!("The specified task definition does not exist: {td_id}"),
        )
    })?;

    let td = if let Some(rev) = maybe_rev {
        let idx = (rev - 1) as usize;
        revisions.get(idx).ok_or_else(|| {
            AwsError::bad_request(
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
                AwsError::bad_request(
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

#[cfg(test)]
mod fargate_cpu_memory_tests {
    use super::*;

    #[test]
    fn accepts_documented_pairs() {
        validate_fargate_cpu_memory("256", "512").unwrap();
        validate_fargate_cpu_memory("256", "2048").unwrap();
        validate_fargate_cpu_memory("1024", "8192").unwrap();
        validate_fargate_cpu_memory("16384", "122880").unwrap();
    }

    #[test]
    fn rejects_invalid_cpu() {
        let err = validate_fargate_cpu_memory("300", "1024").unwrap_err();
        assert_eq!(err.code, "ClientException");
        assert!(err.message.contains("cpu"));
    }

    #[test]
    fn rejects_memory_outside_cpu_band() {
        let err = validate_fargate_cpu_memory("256", "3072").unwrap_err();
        assert_eq!(err.code, "ClientException");
        assert!(err.message.contains("memory"));
    }

    #[test]
    fn rejects_non_numeric_cpu_or_memory() {
        assert!(validate_fargate_cpu_memory("xyz", "512").is_err());
        assert!(validate_fargate_cpu_memory("256", "xyz").is_err());
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    use crate::state::EcsState;

    fn ctx() -> RequestContext {
        RequestContext::new("ecs", "us-east-1")
    }

    fn base_input() -> Value {
        json!({
            "family": "t",
            "containerDefinitions": [],
        })
    }

    #[test]
    fn persists_placement_constraints_and_strategy() {
        let state = EcsState::default();
        let mut input = base_input();
        input["placementConstraints"] = json!([{ "type": "memberOf", "expression": "attribute:ecs.instance-type == t3.medium" }]);
        input["placementStrategy"] =
            json!([{ "type": "spread", "field": "attribute:ecs.availability-zone" }]);
        let resp = register_task_definition(&state, &input, &ctx()).unwrap();
        let td = &resp["taskDefinition"];
        assert_eq!(td["placementConstraints"][0]["type"], "memberOf");
        assert_eq!(td["placementStrategy"][0]["type"], "spread");
    }

    #[test]
    fn rejects_unknown_placement_constraint_type() {
        let state = EcsState::default();
        let mut input = base_input();
        input["placementConstraints"] = json!([{ "type": "bogus" }]);
        let err = register_task_definition(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "ClientException");
    }

    #[test]
    fn rejects_unknown_placement_strategy_type() {
        let state = EcsState::default();
        let mut input = base_input();
        input["placementStrategy"] = json!([{ "type": "bogus" }]);
        let err = register_task_definition(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "ClientException");
    }
}
