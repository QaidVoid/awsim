use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::clusters::{now_epoch_str, resolve_cluster_name};
use crate::state::{EcsState, Service};

fn service_to_json(svc: &Service) -> Value {
    json!({
        "serviceArn": svc.service_arn,
        "serviceName": svc.service_name,
        "clusterArn": svc.cluster_arn,
        "taskDefinition": svc.task_definition,
        "desiredCount": svc.desired_count,
        "runningCount": svc.running_count,
        "pendingCount": 0,
        "status": svc.status,
        "launchType": svc.launch_type,
        "createdAt": svc.created_at,
        "deployments": [],
        "events": [],
        "loadBalancers": [],
        "serviceRegistries": [],
        "networkConfiguration": null,
    })
}

// ---------------------------------------------------------------------------
// CreateService
// ---------------------------------------------------------------------------

pub fn create_service(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id).to_string();

    let service_name = input["serviceName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "serviceName is required")
        })?
        .to_string();

    let task_definition = input["taskDefinition"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "taskDefinition is required")
        })?
        .to_string();

    let desired_count = input["desiredCount"].as_i64().unwrap_or(1);
    let launch_type = input["launchType"].as_str().unwrap_or("EC2").to_string();

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let cluster_arn = cluster.arn.clone();
    let service_arn = format!(
        "arn:aws:ecs:{}:{}:service/{}/{}",
        ctx.region, ctx.account_id, cluster_name, service_name
    );

    let service = Service {
        service_name: service_name.clone(),
        service_arn: service_arn.clone(),
        cluster_arn,
        task_definition,
        desired_count,
        running_count: 0,
        status: "ACTIVE".to_string(),
        launch_type,
        created_at: now_epoch_str(),
    };

    info!(cluster = %cluster_name, service = %service_name, "Created ECS service");
    let svc_json = service_to_json(&service);
    cluster.services.insert(service_name, service);

    Ok(json!({ "service": svc_json }))
}

// ---------------------------------------------------------------------------
// DeleteService
// ---------------------------------------------------------------------------

pub fn delete_service(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let service_id = input["service"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "service is required"))?;

    // service can be name or ARN
    let service_name = if service_id.starts_with("arn:") {
        service_id.split('/').last().unwrap_or(service_id)
    } else {
        service_id
    };

    let mut cluster = state.clusters.get_mut(cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let svc = cluster.services.remove(service_name).ok_or_else(|| {
        AwsError::not_found(
            "ServiceNotFoundException",
            format!("The specified service '{service_name}' does not exist"),
        )
    })?;

    info!(cluster = %cluster_name, service = %service_name, "Deleted ECS service");

    Ok(json!({ "service": service_to_json(&svc) }))
}

// ---------------------------------------------------------------------------
// DescribeServices
// ---------------------------------------------------------------------------

pub fn describe_services(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let service_ids = input["services"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "services is required")
    })?;

    let cluster = state.clusters.get(cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let mut services = Vec::new();
    let mut failures = Vec::new();

    for id_val in service_ids {
        let id = id_val.as_str().unwrap_or("");
        let name = if id.starts_with("arn:") {
            id.split('/').last().unwrap_or(id)
        } else {
            id
        };

        match cluster.services.get(name) {
            Some(svc) => services.push(service_to_json(svc)),
            None => failures.push(json!({
                "arn": id,
                "reason": "MISSING",
                "detail": format!("Service '{name}' not found in cluster '{cluster_name}'"),
            })),
        }
    }

    Ok(json!({ "services": services, "failures": failures }))
}

// ---------------------------------------------------------------------------
// ListServices
// ---------------------------------------------------------------------------

pub fn list_services(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let cluster = state.clusters.get(cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let arns: Vec<Value> = cluster
        .services
        .values()
        .map(|svc| json!(svc.service_arn))
        .collect();

    Ok(json!({ "serviceArns": arns }))
}

// ---------------------------------------------------------------------------
// UpdateService
// ---------------------------------------------------------------------------

pub fn update_service(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id).to_string();

    let service_id = input["service"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "service is required"))?;

    let service_name = if service_id.starts_with("arn:") {
        service_id
            .split('/')
            .last()
            .unwrap_or(service_id)
            .to_string()
    } else {
        service_id.to_string()
    };

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let svc = cluster.services.get_mut(&service_name).ok_or_else(|| {
        AwsError::not_found(
            "ServiceNotFoundException",
            format!("The specified service '{service_name}' does not exist"),
        )
    })?;

    if let Some(td) = input["taskDefinition"].as_str() {
        svc.task_definition = td.to_string();
    }
    if let Some(count) = input["desiredCount"].as_i64() {
        svc.desired_count = count;
    }

    info!(cluster = %cluster_name, service = %service_name, "Updated ECS service");

    Ok(json!({ "service": service_to_json(svc) }))
}
