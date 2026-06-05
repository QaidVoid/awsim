use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::clusters::{epoch_number, now_epoch_str, resolve_cluster_name};
use crate::state::{EcsState, Service};

fn service_to_json(svc: &Service) -> Value {
    let tags: Vec<Value> = svc
        .tags
        .iter()
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect();
    let mut obj = json!({
        "serviceArn": svc.service_arn,
        "serviceName": svc.service_name,
        "clusterArn": svc.cluster_arn,
        "taskDefinition": svc.task_definition,
        "desiredCount": svc.desired_count,
        "runningCount": svc.running_count,
        "pendingCount": 0,
        "status": svc.status,
        "launchType": svc.launch_type,
        "createdAt": epoch_number(&svc.created_at),
        "deployments": [],
        "events": [],
        "loadBalancers": svc.load_balancers,
        "serviceRegistries": svc.service_registries,
        "networkConfiguration": svc.network_configuration.clone().unwrap_or(Value::Null),
        "tags": tags,
        "enableECSManagedTags": svc.enable_ecs_managed_tags,
    });
    if let Some(dc) = &svc.deployment_configuration {
        obj["deploymentConfiguration"] = dc.clone();
    }
    if let Some(dc) = &svc.deployment_controller {
        obj["deploymentController"] = dc.clone();
    }
    if let Some(p) = &svc.propagate_tags {
        obj["propagateTags"] = json!(p);
    }
    obj
}

// ---------------------------------------------------------------------------
// CreateService
// ---------------------------------------------------------------------------

pub fn create_service(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
    cloudmap_registrar: Option<&dyn awsim_core::CloudMapRegistrar>,
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

    let load_balancers: Vec<Value> = input["loadBalancers"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let deployment_configuration = match input.get("deploymentConfiguration") {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => None,
    };

    let deployment_controller = match input.get("deploymentController") {
        Some(dc) if !dc.is_null() => {
            let ty = dc["type"].as_str().unwrap_or("");
            if !matches!(ty, "ECS" | "CODE_DEPLOY" | "EXTERNAL") {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!(
                        "deploymentController.type `{ty}` must be ECS, CODE_DEPLOY, or EXTERNAL."
                    ),
                ));
            }
            Some(dc.clone())
        }
        _ => None,
    };

    let network_configuration = match input.get("networkConfiguration") {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => None,
    };

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let cluster_arn = cluster.arn.clone();
    let service_arn = arn::build(ctx, "ecs", format!("service/{cluster_name}/{service_name}"));

    let tags = crate::operations::tags::parse_tags(input.get("tags"));
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

    let service_registries: Vec<Value> = input
        .get("serviceRegistries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(registrar) = cloudmap_registrar {
        for entry in &service_registries {
            let Some(registry_arn) = entry.get("registryArn").and_then(Value::as_str) else {
                continue;
            };
            let mut attrs = std::collections::HashMap::new();
            if let Some(port) = entry.get("port").and_then(Value::as_i64) {
                attrs.insert("AWS_INSTANCE_PORT".to_string(), port.to_string());
            }
            attrs.insert("ECS_SERVICE_NAME".to_string(), service_name.clone());
            attrs.insert("ECS_CLUSTER_NAME".to_string(), cluster_name.clone());
            if !registrar.register_instance(
                registry_arn,
                &service_arn,
                &attrs,
                &ctx.account_id,
                &ctx.region,
            ) {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!(
                        "serviceRegistries[].registryArn `{registry_arn}` does not resolve to a Cloud Map service."
                    ),
                ));
            }
        }
    }

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
        load_balancers,
        deployment_configuration,
        deployment_controller,
        network_configuration,
        tags,
        propagate_tags,
        enable_ecs_managed_tags,
        service_registries,
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
    ctx: &RequestContext,
    cloudmap_registrar: Option<&dyn awsim_core::CloudMapRegistrar>,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let service_id = input["service"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "service is required"))?;

    // service can be name or ARN
    let service_name = if service_id.starts_with("arn:") {
        service_id.split('/').next_back().unwrap_or(service_id)
    } else {
        service_id
    };

    let mut cluster = state.clusters.get_mut(cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let svc = cluster.services.remove(service_name).ok_or_else(|| {
        AwsError::bad_request(
            "ServiceNotFoundException",
            format!("The specified service '{service_name}' does not exist"),
        )
    })?;

    if let Some(registrar) = cloudmap_registrar {
        for entry in &svc.service_registries {
            if let Some(registry_arn) = entry.get("registryArn").and_then(Value::as_str) {
                registrar.deregister_instance(
                    registry_arn,
                    &svc.service_arn,
                    &ctx.account_id,
                    &ctx.region,
                );
            }
        }
    }

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
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let mut services = Vec::new();
    let mut failures = Vec::new();

    for id_val in service_ids {
        let id = id_val.as_str().unwrap_or("");
        let name = if id.starts_with("arn:") {
            id.split('/').next_back().unwrap_or(id)
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
        AwsError::bad_request(
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
            .next_back()
            .unwrap_or(service_id)
            .to_string()
    } else {
        service_id.to_string()
    };

    let mut cluster = state.clusters.get_mut(&cluster_name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{cluster_name}' does not exist"),
        )
    })?;

    let svc = cluster.services.get_mut(&service_name).ok_or_else(|| {
        AwsError::bad_request(
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

#[cfg(test)]
mod cloudmap_tests {
    use super::*;
    use crate::operations::clusters::create_cluster;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn ctx() -> RequestContext {
        RequestContext::new("ecs", "us-east-1")
    }

    #[derive(Default)]
    struct StubRegistrar {
        known: HashMap<String, ()>,
        registered: Mutex<Vec<(String, String)>>,
        deregistered: Mutex<Vec<(String, String)>>,
    }

    impl awsim_core::CloudMapRegistrar for StubRegistrar {
        fn register_instance(
            &self,
            registry_arn: &str,
            instance_id: &str,
            _attributes: &HashMap<String, String>,
            _account: &str,
            _region: &str,
        ) -> bool {
            if !self.known.contains_key(registry_arn) {
                return false;
            }
            self.registered
                .lock()
                .unwrap()
                .push((registry_arn.to_string(), instance_id.to_string()));
            true
        }
        fn deregister_instance(
            &self,
            registry_arn: &str,
            instance_id: &str,
            _account: &str,
            _region: &str,
        ) {
            self.deregistered
                .lock()
                .unwrap()
                .push((registry_arn.to_string(), instance_id.to_string()));
        }
    }

    fn registrar_with(registry_arn: &str) -> StubRegistrar {
        let mut r = StubRegistrar::default();
        r.known.insert(registry_arn.to_string(), ());
        r
    }

    #[test]
    fn create_service_registers_cloudmap_instance_per_registry() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let registry = "arn:aws:servicediscovery:us-east-1:000000000000:service/srv-abc";
        let registrar = registrar_with(registry);

        create_service(
            &state,
            &json!({
                "cluster": "default",
                "serviceName": "api",
                "taskDefinition": "web",
                "serviceRegistries": [{ "registryArn": registry, "port": 8080 }],
            }),
            &ctx(),
            Some(&registrar),
        )
        .unwrap();

        let registered = registrar.registered.lock().unwrap();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].0, registry);
    }

    #[test]
    fn create_service_rejects_unknown_registry() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let registrar = StubRegistrar::default();
        let err = create_service(
            &state,
            &json!({
                "cluster": "default",
                "serviceName": "api",
                "taskDefinition": "web",
                "serviceRegistries": [{
                    "registryArn": "arn:aws:servicediscovery:us-east-1:000000000000:service/missing",
                }],
            }),
            &ctx(),
            Some(&registrar),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn delete_service_deregisters_each_registry() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let registry = "arn:aws:servicediscovery:us-east-1:000000000000:service/srv-abc";
        let registrar = registrar_with(registry);

        create_service(
            &state,
            &json!({
                "cluster": "default",
                "serviceName": "api",
                "taskDefinition": "web",
                "serviceRegistries": [{ "registryArn": registry }],
            }),
            &ctx(),
            Some(&registrar),
        )
        .unwrap();
        delete_service(
            &state,
            &json!({ "cluster": "default", "service": "api" }),
            &ctx(),
            Some(&registrar),
        )
        .unwrap();

        let deregistered = registrar.deregistered.lock().unwrap();
        assert_eq!(deregistered.len(), 1);
        assert_eq!(deregistered[0].0, registry);
    }

    #[test]
    fn create_service_without_registrar_persists_registries_unvalidated() {
        let state = EcsState::default();
        create_cluster(&state, &json!({ "clusterName": "default" }), &ctx()).unwrap();
        let registry = "arn:aws:servicediscovery:us-east-1:000000000000:service/anything";
        let resp = create_service(
            &state,
            &json!({
                "cluster": "default",
                "serviceName": "api",
                "taskDefinition": "web",
                "serviceRegistries": [{ "registryArn": registry }],
            }),
            &ctx(),
            None,
        )
        .unwrap();
        let regs = resp["service"]["serviceRegistries"].as_array().unwrap();
        assert_eq!(regs[0]["registryArn"], registry);
    }
}
