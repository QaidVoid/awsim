use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Cluster, EcsState};

pub fn now_epoch_str() -> String {
    now_epoch().to_string()
}

/// Current time as epoch seconds, for emitting awsJson1.1 timestamp members
/// (which must be JSON numbers, not strings).
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Convert a stored timestamp string to a JSON number of epoch seconds for
/// awsJson1.1 responses. Accepts an epoch-seconds string or an ISO-8601 value
/// (e.g. `2024-01-01T00:00:00Z`); falls back to `0` if unparseable.
pub fn epoch_number(stored: &str) -> Value {
    if let Ok(secs) = stored.parse::<u64>() {
        return json!(secs);
    }
    if let Some(secs) = parse_iso8601_epoch(stored) {
        return json!(secs);
    }
    json!(0)
}

/// Parse a minimal ISO-8601 UTC timestamp `YYYY-MM-DDTHH:MM:SSZ` to epoch seconds.
fn parse_iso8601_epoch(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z').unwrap_or(s);
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").split('.').next()?.parse().ok()?;

    // Days since 1970-01-01 via the civil-from-days algorithm.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    u64::try_from(secs).ok()
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
        input_cluster
            .split('/')
            .next_back()
            .unwrap_or(input_cluster)
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
    let name = input["clusterName"]
        .as_str()
        .unwrap_or("default")
        .to_string();

    let arn = arn::build(ctx, "ecs", format!("cluster/{name}"));

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
        AwsError::bad_request(
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
        let all: Vec<Value> = state
            .clusters
            .iter()
            .map(|e| cluster_to_json(e.value()))
            .collect();
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
