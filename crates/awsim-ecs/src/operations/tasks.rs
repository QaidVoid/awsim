use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::operations::clusters::{epoch_number, now_epoch_str, resolve_cluster_name};
use crate::state::{EcsState, Task};

fn task_to_json(task: &Task) -> Value {
    let tags: Vec<Value> = task
        .tags
        .iter()
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect();
    json!({
        "taskArn": task.task_arn,
        "clusterArn": task.cluster_arn,
        "taskDefinitionArn": task.task_definition_arn,
        "lastStatus": task.status,
        "desiredStatus": task.status,
        "group": task.group,
        "startedAt": epoch_number(&task.started_at),
        "containers": [],
        "attachments": task.attachments,
        "attributes": [],
        "tags": tags,
    })
}

/// Build the ECS `ElasticNetworkInterface` attachment record for an
/// `awsvpc` task. The simulator doesn't model EC2 ENIs end-to-end,
/// but every field AWS ships back on describe is here:
///   - `id`: attachment uuid
///   - `networkInterfaceId`: synthetic `eni-{12-hex}`
///   - `subnetId`: pulled from `networkConfiguration.awsvpcConfiguration.subnets[0]`
///   - `privateIPv4Address`: derived from the eni-id so the same call
///     deterministically yields the same address
///   - `securityGroups`: comma-joined input list (or empty)
fn build_awsvpc_attachment(network_configuration: &Value) -> Value {
    let aws_cfg = network_configuration
        .get("awsvpcConfiguration")
        .cloned()
        .unwrap_or(Value::Null);

    let subnet = aws_cfg
        .get("subnets")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .unwrap_or("subnet-awsim-default")
        .to_string();

    let security_groups = aws_cfg
        .get("securityGroups")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let raw = Uuid::new_v4().to_string().replace('-', "");
    let eni_id = format!("eni-{}", &raw[..12]);
    // Derive a fake 10.0.x.y from the eni id so describe responses
    // round-trip the same address for the same attachment id.
    let bytes = raw.as_bytes();
    let octet_a = (u32::from(bytes[0]) + u32::from(bytes[1])) % 256;
    let octet_b = (u32::from(bytes[2]) + u32::from(bytes[3])) % 254 + 1;
    let private_ip = format!("10.0.{octet_a}.{octet_b}");

    let mut details = vec![
        json!({"name": "subnetId", "value": subnet}),
        json!({"name": "networkInterfaceId", "value": eni_id}),
        json!({"name": "macAddress", "value": "0a:00:00:00:00:01"}),
        json!({"name": "privateIPv4Address", "value": private_ip}),
    ];
    if !security_groups.is_empty() {
        details.push(json!({"name": "securityGroups", "value": security_groups}));
    }

    json!({
        "id": Uuid::new_v4().to_string(),
        "type": "ElasticNetworkInterface",
        "status": "ATTACHED",
        "details": details,
    })
}

/// Walk every container's `secrets[]` entry on the task definition
/// and confirm each `valueFrom` resolves to an existing
/// SecretsManager secret or SSM parameter. AWS rejects RunTask when
/// any reference fails to resolve; the simulator mirrors that with
/// `ClientException` when a lookup is wired. Missing lookups are a
/// no-op so test setups that skip cross-service plumbing keep
/// working.
fn validate_container_secrets(
    state: &EcsState,
    task_def_arn: &str,
    ctx: &RequestContext,
    secrets_lookup: Option<&dyn awsim_core::SecretLookup>,
    parameters_lookup: Option<&dyn awsim_core::ParameterLookup>,
) -> Result<(), AwsError> {
    // Find the task definition for the supplied ARN. RunTask should
    // already have resolved it; if we can't find it here, skip
    // validation rather than double-rejecting on a missing TD.
    let containers = {
        let mut found: Option<Value> = None;
        for entry in state.task_definitions.iter() {
            for td in entry.value() {
                if td.arn == task_def_arn {
                    found = Some(td.container_definitions.clone());
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        match found {
            Some(c) => c,
            None => return Ok(()),
        }
    };
    let Some(arr) = containers.as_array() else {
        return Ok(());
    };

    for container in arr {
        let Some(secrets) = container.get("secrets").and_then(Value::as_array) else {
            continue;
        };
        for secret in secrets {
            let name = secret
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<unnamed>");
            let value_from = secret
                .get("valueFrom")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AwsError::bad_request(
                        "ClientException",
                        format!("Container secret `{name}` must specify valueFrom."),
                    )
                })?;

            let (account, region) = parse_aws_arn_account_region(value_from)
                .unwrap_or_else(|| (ctx.account_id.clone(), ctx.region.clone()));

            let kind = classify_secret_reference(value_from);
            match kind {
                SecretRefKind::SecretsManager => {
                    if let Some(lookup) = secrets_lookup
                        && !lookup.secret_exists(value_from, &account, &region)
                    {
                        return Err(AwsError::bad_request(
                            "ClientException",
                            format!(
                                "Container secret `{name}` references SecretsManager secret \
                                 `{value_from}` which does not exist."
                            ),
                        ));
                    }
                }
                SecretRefKind::Ssm => {
                    if let Some(lookup) = parameters_lookup
                        && !lookup.parameter_exists(value_from, &account, &region)
                    {
                        return Err(AwsError::bad_request(
                            "ClientException",
                            format!(
                                "Container secret `{name}` references SSM parameter \
                                 `{value_from}` which does not exist."
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretRefKind {
    SecretsManager,
    Ssm,
}

/// Classify a container `secrets[].valueFrom` value. AWS accepts:
///   - `arn:aws:secretsmanager:...` → SecretsManager
///   - `arn:aws:ssm:...:parameter/...` → SSM Parameter Store
///   - Plain parameter name (e.g. `/myapp/db-pass`) → SSM
fn classify_secret_reference(value_from: &str) -> SecretRefKind {
    if value_from.starts_with("arn:aws:secretsmanager:") {
        SecretRefKind::SecretsManager
    } else {
        SecretRefKind::Ssm
    }
}

/// Extract `(account, region)` from any well-formed AWS ARN. Returns
/// `None` for plain parameter names or malformed inputs.
fn parse_aws_arn_account_region(arn: &str) -> Option<(String, String)> {
    let rest = arn.strip_prefix("arn:aws:")?;
    let mut parts = rest.splitn(5, ':');
    let _service = parts.next()?;
    let region = parts.next()?;
    let account = parts.next()?;
    if region.is_empty() || account.is_empty() {
        return None;
    }
    Some((account.to_string(), region.to_string()))
}

/// Look up the network mode declared on a task definition by ARN.
/// Returns `None` when the definition can't be resolved — RunTask
/// falls back to "no awsvpc handling" in that case rather than
/// rejecting the call.
fn task_definition_network_mode(state: &EcsState, task_def_arn: &str) -> Option<String> {
    for entry in state.task_definitions.iter() {
        for td in entry.value().iter() {
            if td.arn == task_def_arn {
                return Some(td.network_mode.clone());
            }
        }
    }
    None
}

/// Pull tags off the matching `TaskDefinition` so `propagateTags=
/// TASK_DEFINITION` can copy them onto each spawned task. Falls back
/// to an empty list when the definition is unknown.
fn task_definition_tags(state: &EcsState, task_def_arn: &str) -> Vec<(String, String)> {
    for entry in state.task_definitions.iter() {
        for td in entry.value().iter() {
            if td.arn == task_def_arn {
                return td.tags.clone();
            }
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// RunTask
// ---------------------------------------------------------------------------

pub fn run_task(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
    secrets_lookup: Option<&dyn awsim_core::SecretLookup>,
    parameters_lookup: Option<&dyn awsim_core::ParameterLookup>,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id).to_string();

    let task_definition = input["taskDefinition"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "taskDefinition is required")
        })?
        .to_string();

    let count = input["count"].as_u64().unwrap_or(1);

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let cluster_arn = cluster.arn.clone();

    // Resolve task definition ARN
    let task_def_arn = if task_definition.starts_with("arn:") {
        task_definition.clone()
    } else {
        // family:revision or family — look up
        let (family, maybe_rev) =
            crate::operations::task_definitions::parse_task_definition_id(&task_definition);
        match state.task_definitions.get(family) {
            Some(revisions) => {
                let td = if let Some(rev) = maybe_rev {
                    revisions.get((rev - 1) as usize)
                } else {
                    revisions.iter().rev().find(|td| td.status == "ACTIVE")
                };
                td.map(|t| t.arn.clone()).unwrap_or(task_definition.clone())
            }
            None => task_definition.clone(),
        }
    };

    let propagate_tags = input["propagateTags"].as_str().map(str::to_string);
    if let Some(ref p) = propagate_tags
        && !matches!(p.as_str(), "TASK_DEFINITION" | "SERVICE" | "NONE")
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("propagateTags '{p}' must be one of TASK_DEFINITION, SERVICE, NONE."),
        ));
    }
    let enable_ecs_managed_tags = input["enableECSManagedTags"].as_bool().unwrap_or(false);
    let caller_tags = crate::operations::tags::parse_tags(input.get("tags"));
    let task_def_tags = task_definition_tags(state, &task_def_arn);
    let propagated = match propagate_tags.as_deref() {
        Some("TASK_DEFINITION") => task_def_tags,
        _ => Vec::new(),
    };
    let mut effective_tags = crate::operations::tags::merge_tags(&propagated, &caller_tags);
    if enable_ecs_managed_tags {
        let managed = crate::operations::tags::ecs_managed_tags(&cluster_name, None);
        effective_tags = crate::operations::tags::merge_tags(&effective_tags, &managed);
    }

    // Validate every container's `secrets[]` entry resolves to a real
    // SecretsManager secret or SSM parameter. AWS surfaces the failure
    // at RunTask time (the task never transitions out of PROVISIONING)
    // so we reject up front with ClientException.
    validate_container_secrets(state, &task_def_arn, ctx, secrets_lookup, parameters_lookup)?;

    let network_mode = task_definition_network_mode(state, &task_def_arn);
    let requires_network_config = network_mode.as_deref() == Some("awsvpc");
    let network_configuration = input.get("networkConfiguration").cloned();
    if requires_network_config {
        let has_config = network_configuration
            .as_ref()
            .and_then(|nc| nc.get("awsvpcConfiguration"))
            .and_then(|c| c.get("subnets").and_then(Value::as_array))
            .is_some_and(|s| !s.is_empty());
        if !has_config {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "Tasks using the awsvpc network mode require \
                 networkConfiguration.awsvpcConfiguration.subnets.",
            ));
        }
    }

    let mut tasks = Vec::new();

    for _ in 0..count {
        let task_id = Uuid::new_v4().to_string();
        let task_arn = arn::build(ctx, "ecs", format!("task/{cluster_name}/{task_id}"));

        let attachments = if requires_network_config {
            let nc = network_configuration.clone().unwrap_or(Value::Null);
            vec![build_awsvpc_attachment(&nc)]
        } else {
            Vec::new()
        };

        let task = Task {
            task_arn: task_arn.clone(),
            cluster_arn: cluster_arn.clone(),
            task_definition_arn: task_def_arn.clone(),
            status: "RUNNING".to_string(),
            started_at: now_epoch_str(),
            group: "task-group".to_string(),
            tags: effective_tags.clone(),
            attachments,
        };

        tasks.push(task_to_json(&task));
        cluster.tasks.insert(task_arn, task);
    }

    info!(cluster = %cluster_name, count = count, "Ran ECS tasks");

    Ok(json!({ "tasks": tasks, "failures": [] }))
}

// ---------------------------------------------------------------------------
// StopTask
// ---------------------------------------------------------------------------

pub fn stop_task(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id).to_string();

    let task_id = input["task"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "task is required"))?;

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    // task can be ARN or short ID
    let task_arn = if task_id.starts_with("arn:") {
        task_id.to_string()
    } else {
        // find by suffix
        cluster
            .tasks
            .keys()
            .find(|k| k.ends_with(task_id))
            .cloned()
            .unwrap_or_else(|| task_id.to_string())
    };

    let task = cluster.tasks.get_mut(&task_arn).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("The specified task '{task_id}' does not exist"),
        )
    })?;

    task.status = "STOPPED".to_string();
    let task_json = task_to_json(task);

    info!(cluster = %cluster_name, task = %task_arn, "Stopped ECS task");

    Ok(json!({ "task": task_json }))
}

// ---------------------------------------------------------------------------
// DescribeTasks
// ---------------------------------------------------------------------------

pub fn describe_tasks(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let task_ids = input["tasks"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "tasks is required"))?;

    let cluster = state.clusters.get(cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let mut tasks = Vec::new();
    let mut failures = Vec::new();

    for id_val in task_ids {
        let id = id_val.as_str().unwrap_or("");
        let found = if id.starts_with("arn:") {
            cluster.tasks.get(id)
        } else {
            cluster
                .tasks
                .iter()
                .find(|(k, _)| k.ends_with(id))
                .map(|(_, v)| v)
        };

        match found {
            Some(task) => tasks.push(task_to_json(task)),
            None => failures.push(json!({
                "arn": id,
                "reason": "MISSING",
                "detail": format!("Task '{id}' not found"),
            })),
        }
    }

    Ok(json!({ "tasks": tasks, "failures": failures }))
}

// ---------------------------------------------------------------------------
// ListTasks
// ---------------------------------------------------------------------------

pub fn list_tasks(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let service_name_filter = input["serviceName"].as_str();
    let family_filter = input["family"].as_str();

    let cluster = state.clusters.get(cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let arns: Vec<Value> = cluster
        .tasks
        .values()
        .filter(|task| {
            // Filter by service group if requested
            if let Some(svc) = service_name_filter
                && !task.group.contains(svc)
            {
                return false;
            }
            // Filter by family if requested
            if let Some(family) = family_filter
                && !task.task_definition_arn.contains(family)
            {
                return false;
            }
            true
        })
        .map(|task| json!(task.task_arn))
        .collect();

    Ok(json!({ "taskArns": arns }))
}

#[cfg(test)]
mod propagate_tags_tests {
    use super::*;
    use crate::operations::clusters::create_cluster;
    use crate::operations::task_definitions::register_task_definition;

    fn ctx() -> RequestContext {
        RequestContext::new("ecs", "us-east-1")
    }

    #[test]
    fn run_task_propagates_task_definition_tags_when_enabled() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_task_definition(
            &state,
            &json!({
                "family": "web",
                "containerDefinitions": [],
                "tags": [
                    { "key": "team", "value": "data" },
                    { "key": "env", "value": "prod" }
                ]
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let resp = run_task(
            &state,
            &json!({
                "cluster": "default",
                "taskDefinition": "web",
                "propagateTags": "TASK_DEFINITION",
                "tags": [{ "key": "team", "value": "override" }]
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let tags = resp["tasks"][0]["tags"].as_array().unwrap();
        let env = tags.iter().find(|t| t["key"] == "env").unwrap();
        assert_eq!(env["value"], "prod");
        // Caller override wins on key collision.
        let team = tags.iter().find(|t| t["key"] == "team").unwrap();
        assert_eq!(team["value"], "override");
    }

    #[test]
    fn run_task_attaches_ecs_managed_tags_when_enabled() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_task_definition(
            &state,
            &json!({ "family": "web", "containerDefinitions": [] }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let resp = run_task(
            &state,
            &json!({
                "cluster": "default",
                "taskDefinition": "web",
                "enableECSManagedTags": true,
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let tags = resp["tasks"][0]["tags"].as_array().unwrap();
        assert!(
            tags.iter()
                .any(|t| t["key"] == "aws:ecs:clusterName" && t["value"] == "default")
        );
    }

    #[test]
    fn run_task_rejects_invalid_propagate_tags() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_task_definition(
            &state,
            &json!({ "family": "web", "containerDefinitions": [] }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let err = run_task(
            &state,
            &json!({
                "cluster": "default",
                "taskDefinition": "web",
                "propagateTags": "MYSTERY"
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    fn register_awsvpc_def(state: &EcsState) {
        register_task_definition(
            state,
            &json!({
                "family": "web",
                "containerDefinitions": [],
                "networkMode": "awsvpc"
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn run_task_attaches_eni_for_awsvpc_network_mode() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_awsvpc_def(&state);

        let resp = run_task(
            &state,
            &json!({
                "cluster": "default",
                "taskDefinition": "web",
                "networkConfiguration": {
                    "awsvpcConfiguration": {
                        "subnets": ["subnet-aaa"],
                        "securityGroups": ["sg-1", "sg-2"]
                    }
                }
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();

        let attachments = resp["tasks"][0]["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 1);
        let att = &attachments[0];
        assert_eq!(att["type"], "ElasticNetworkInterface");
        assert_eq!(att["status"], "ATTACHED");
        let details = att["details"].as_array().unwrap();
        let subnet = details.iter().find(|d| d["name"] == "subnetId").unwrap();
        assert_eq!(subnet["value"], "subnet-aaa");
        let eni = details
            .iter()
            .find(|d| d["name"] == "networkInterfaceId")
            .unwrap();
        assert!(eni["value"].as_str().unwrap().starts_with("eni-"));
        let sg = details
            .iter()
            .find(|d| d["name"] == "securityGroups")
            .unwrap();
        assert_eq!(sg["value"], "sg-1,sg-2");
    }

    #[test]
    fn run_task_rejects_awsvpc_without_network_configuration() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_awsvpc_def(&state);
        let err = run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    struct StubSecrets {
        known: std::collections::HashSet<String>,
    }
    impl awsim_core::SecretLookup for StubSecrets {
        fn secret_exists(&self, secret_ref: &str, _: &str, _: &str) -> bool {
            self.known.contains(secret_ref)
        }
    }
    struct StubParameters {
        known: std::collections::HashSet<String>,
    }
    impl awsim_core::ParameterLookup for StubParameters {
        fn parameter_exists(&self, parameter_ref: &str, _: &str, _: &str) -> bool {
            self.known.contains(parameter_ref)
        }
    }

    fn register_with_container_secrets(state: &EcsState, value_from: &str) {
        register_task_definition(
            state,
            &json!({
                "family": "web",
                "containerDefinitions": [{
                    "name": "app",
                    "image": "img",
                    "secrets": [{ "name": "DB_PASS", "valueFrom": value_from }]
                }]
            }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn run_task_rejects_missing_secretsmanager_secret() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let arn = "arn:aws:secretsmanager:us-east-1:000000000000:secret:db-pass-abc123";
        register_with_container_secrets(&state, arn);
        let lookup = StubSecrets {
            known: std::collections::HashSet::new(),
        };
        let err = run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            Some(&lookup),
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("SecretsManager"), "{err:?}");
    }

    #[test]
    fn run_task_accepts_resolvable_secretsmanager_secret() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let arn = "arn:aws:secretsmanager:us-east-1:000000000000:secret:db-pass-abc123";
        register_with_container_secrets(&state, arn);
        let mut known = std::collections::HashSet::new();
        known.insert(arn.to_string());
        let lookup = StubSecrets { known };
        run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            Some(&lookup),
            None,
        )
        .unwrap();
    }

    #[test]
    fn run_task_rejects_missing_ssm_parameter() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_with_container_secrets(&state, "/myapp/db");
        let lookup = StubParameters {
            known: std::collections::HashSet::new(),
        };
        let err = run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            None,
            Some(&lookup),
        )
        .unwrap_err();
        assert!(err.message.contains("SSM parameter"), "{err:?}");
    }

    #[test]
    fn run_task_accepts_resolvable_ssm_parameter() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_with_container_secrets(&state, "/myapp/db");
        let mut known = std::collections::HashSet::new();
        known.insert("/myapp/db".to_string());
        let lookup = StubParameters { known };
        run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            None,
            Some(&lookup),
        )
        .unwrap();
    }

    #[test]
    fn run_task_skips_secret_validation_when_no_lookup_wired() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_with_container_secrets(
            &state,
            "arn:aws:secretsmanager:us-east-1:000000000000:secret:never-validated",
        );
        run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn run_task_bridge_mode_emits_empty_attachments() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        register_task_definition(
            &state,
            &json!({ "family": "web", "containerDefinitions": [] }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let resp = run_task(
            &state,
            &json!({ "cluster": "default", "taskDefinition": "web" }),
            &ctx(),
            None,
            None,
        )
        .unwrap();
        let attachments = resp["tasks"][0]["attachments"].as_array().unwrap();
        assert!(attachments.is_empty());
    }
}
