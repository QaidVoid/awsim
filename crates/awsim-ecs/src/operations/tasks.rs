use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::operations::clusters::{now_epoch_str, resolve_cluster_name};
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
        "startedAt": task.started_at,
        "containers": [],
        "attachments": [],
        "attributes": [],
        "tags": tags,
    })
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

pub fn run_task(state: &EcsState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
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

    let mut tasks = Vec::new();

    for _ in 0..count {
        let task_id = Uuid::new_v4().to_string();
        let task_arn = format!(
            "arn:aws:ecs:{}:{}:task/{}/{}",
            ctx.region, ctx.account_id, cluster_name, task_id
        );

        let task = Task {
            task_arn: task_arn.clone(),
            cluster_arn: cluster_arn.clone(),
            task_definition_arn: task_def_arn.clone(),
            status: "RUNNING".to_string(),
            started_at: now_epoch_str(),
            group: "task-group".to_string(),
            tags: effective_tags.clone(),
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
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }
}
