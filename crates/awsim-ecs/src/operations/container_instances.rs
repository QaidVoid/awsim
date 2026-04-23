use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::EcsState;

use super::clusters::resolve_cluster_name;

fn instance_to_json(arn: &str, cluster_arn: &str) -> Value {
    json!({
        "containerInstanceArn": arn,
        "ec2InstanceId": "i-00000000",
        "capacityProviderName": null,
        "version": 1,
        "versionInfo": {
            "agentVersion": "1.0.0",
            "agentHash": "stub",
            "dockerVersion": "20.10.0",
        },
        "remainingResources": [],
        "registeredResources": [],
        "status": "ACTIVE",
        "agentConnected": true,
        "runningTasksCount": 0,
        "pendingTasksCount": 0,
        "attributes": [],
        "registeredAt": "2024-01-01T00:00:00Z",
        "attachments": [],
        "tags": [],
        "clusterArn": cluster_arn,
    })
}

pub fn describe_container_instances(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let cluster_arn = state
        .clusters
        .get(cluster_name)
        .map(|c| c.arn.clone())
        .unwrap_or_else(|| {
            format!(
                "arn:aws:ecs:{}:{}:cluster/{}",
                ctx.region, ctx.account_id, cluster_name
            )
        });

    let instances: Vec<Value> = input["containerInstances"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|id| {
                    let arn = if id.starts_with("arn:") {
                        id.to_string()
                    } else {
                        format!(
                            "arn:aws:ecs:{}:{}:container-instance/{}/{}",
                            ctx.region, ctx.account_id, cluster_name, id
                        )
                    };
                    instance_to_json(&arn, &cluster_arn)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "containerInstances": instances,
        "failures": [],
    }))
}

pub fn list_container_instances(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let _ = state.clusters.get(cluster_name);

    Ok(json!({
        "containerInstanceArns": Value::Array(vec![]),
        "nextToken": Value::Null,
        "_account": ctx.account_id,
    }))
}

pub fn list_attributes(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let target_type = input["targetType"].as_str().unwrap_or("container-instance");
    let attr_name_filter = input["attributeName"].as_str();
    let attr_value_filter = input["attributeValue"].as_str();

    let key = format!("{}|{}", resolve_cluster_name(cluster_id), target_type);
    let attrs: Vec<Value> = state
        .attributes
        .get(&key)
        .map(|a| {
            a.iter()
                .filter(|(name, value)| {
                    attr_name_filter.map_or(true, |f| name.as_str() == f)
                        && attr_value_filter.map_or(true, |f| value.as_str() == f)
                })
                .map(|(name, value)| {
                    json!({
                        "name": name,
                        "value": value,
                        "targetType": target_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "attributes": attrs,
        "nextToken": Value::Null,
    }))
}

pub fn put_attributes(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let attrs_input = input["attributes"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "attributes is required")
    })?;

    let mut applied: Vec<Value> = Vec::new();
    for attr in attrs_input {
        let name = attr["name"].as_str().unwrap_or("");
        let value = attr["value"].as_str().unwrap_or("").to_string();
        let target_type = attr["targetType"].as_str().unwrap_or("container-instance");
        if name.is_empty() {
            continue;
        }
        let key = format!("{}|{}", cluster_name, target_type);
        let mut entry = state.attributes.entry(key).or_default();
        entry.insert(name.to_string(), value.clone());
        applied.push(json!({
            "name": name,
            "value": value,
            "targetType": target_type,
        }));
    }

    Ok(json!({ "attributes": applied }))
}

pub fn delete_attributes(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = input["cluster"].as_str().unwrap_or("default");
    let cluster_name = resolve_cluster_name(cluster_id);

    let attrs_input = input["attributes"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "attributes is required")
    })?;

    let mut removed: Vec<Value> = Vec::new();
    for attr in attrs_input {
        let name = attr["name"].as_str().unwrap_or("");
        let target_type = attr["targetType"].as_str().unwrap_or("container-instance");
        if name.is_empty() {
            continue;
        }
        let key = format!("{}|{}", cluster_name, target_type);
        let value = if let Some(mut entry) = state.attributes.get_mut(&key) {
            entry.remove(name).unwrap_or_default()
        } else {
            String::new()
        };
        removed.push(json!({
            "name": name,
            "value": value,
            "targetType": target_type,
        }));
    }

    Ok(json!({ "attributes": removed }))
}

pub fn list_services_by_namespace(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let namespace = input["namespace"].as_str().unwrap_or("");
    if namespace.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "namespace is required",
        ));
    }

    let arns: Vec<Value> = state
        .clusters
        .iter()
        .flat_map(|c| {
            c.value()
                .services
                .values()
                .map(|s| json!(s.service_arn))
                .collect::<Vec<_>>()
        })
        .collect();

    Ok(json!({
        "serviceArns": arns,
        "nextToken": Value::Null,
    }))
}
