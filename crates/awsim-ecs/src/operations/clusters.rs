use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Cluster, EcsState};

pub fn now_epoch_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn cluster_to_json(cluster: &Cluster) -> Value {
    json!({
        "clusterArn": cluster.arn,
        "clusterName": cluster.name,
        "status": cluster.status,
        "registeredContainerInstancesCount": 0,
        "runningTasksCount": cluster.tasks.values().filter(|t| t.status == "RUNNING").count(),
        "pendingTasksCount": 0,
        "activeServicesCount": cluster.services.len(),
        "statistics": [],
        "tags": [],
        "capacityProviders": cluster.capacity_providers,
        "defaultCapacityProviderStrategy": cluster.default_capacity_provider_strategy,
    })
}

/// Resolve a cluster identifier (name or ARN) to a cluster name.
pub fn resolve_cluster_name(input_cluster: &str) -> &str {
    // ARN format: arn:aws:ecs:{region}:{account}:cluster/{name}
    if input_cluster.starts_with("arn:") {
        input_cluster.split('/').last().unwrap_or(input_cluster)
    } else {
        input_cluster
    }
}

// ---------------------------------------------------------------------------
// CreateCluster
// ---------------------------------------------------------------------------

pub fn create_cluster(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["clusterName"].as_str().unwrap_or("default").to_string();

    let arn = format!("arn:aws:ecs:{}:{}:cluster/{}", ctx.region, ctx.account_id, name);

    if state.clusters.contains_key(&name) {
        // Idempotent: return existing
        let cluster = state.clusters.get(&name).unwrap();
        return Ok(json!({ "cluster": cluster_to_json(&cluster) }));
    }

    let cluster = Cluster {
        name: name.clone(),
        arn,
        status: "ACTIVE".to_string(),
        services: HashMap::new(),
        tasks: HashMap::new(),
        created_at: now_epoch_str(),
        capacity_providers: Vec::new(),
        default_capacity_provider_strategy: Vec::new(),
    };

    info!(cluster = %name, "Created ECS cluster");
    let json = cluster_to_json(&cluster);
    state.clusters.insert(name, cluster);

    Ok(json!({ "cluster": json }))
}

// ---------------------------------------------------------------------------
// DeleteCluster
// ---------------------------------------------------------------------------

pub fn delete_cluster(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "cluster is required"))?;

    let name = resolve_cluster_name(cluster_id);

    let cluster = state.clusters.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{name}' does not exist"),
        )
    })?;

    let json = cluster_to_json(&cluster);
    drop(cluster);

    state.clusters.remove(name);
    info!(cluster = %name, "Deleted ECS cluster");

    Ok(json!({ "cluster": json }))
}

// ---------------------------------------------------------------------------
// DescribeClusters
// ---------------------------------------------------------------------------

pub fn describe_clusters(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let clusters_input = input["clusters"].as_array();

    let (clusters, failures): (Vec<Value>, Vec<Value>) = if let Some(ids) = clusters_input {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for id_val in ids {
            let id = id_val.as_str().unwrap_or("");
            let name = resolve_cluster_name(id);
            match state.clusters.get(name) {
                Some(c) => found.push(cluster_to_json(&c)),
                None => missing.push(json!({
                    "arn": id,
                    "reason": "MISSING",
                    "detail": format!("Cluster '{name}' not found"),
                })),
            }
        }
        (found, missing)
    } else {
        let all: Vec<Value> = state.clusters.iter().map(|e| cluster_to_json(e.value())).collect();
        (all, vec![])
    };

    Ok(json!({ "clusters": clusters, "failures": failures }))
}

// ---------------------------------------------------------------------------
// ListClusters
// ---------------------------------------------------------------------------

pub fn list_clusters(
    state: &EcsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arns: Vec<Value> = state
        .clusters
        .iter()
        .map(|e| json!(e.value().arn))
        .collect();

    Ok(json!({ "clusterArns": arns }))
}
