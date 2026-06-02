use std::collections::HashMap;

use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::EcsState;

// ---------------------------------------------------------------------------
// TagResource / UntagResource / ListTagsForResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    validate_aws_tags(&input["tags"], &TagOpts::aws_default())?;

    let mut entry = state
        .resource_tags
        .entry(resource_arn.to_string())
        .or_default();

    if let Some(tag_list) = input["tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["key"].as_str(), tag["value"].as_str()) {
                entry.insert(k.to_string(), v.to_string());
            }
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    validate_aws_tag_keys(&input["tagKeys"])?;

    let tag_keys: Vec<&str> = input["tagKeys"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if let Some(mut tags) = state.resource_tags.get_mut(resource_arn) {
        for key in &tag_keys {
            tags.remove(*key);
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "resourceArn is required")
    })?;

    let tags: Vec<Value> = state
        .resource_tags
        .get(resource_arn)
        .map(|t| {
            t.iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({ "tags": tags }))
}

// ---------------------------------------------------------------------------
// PutClusterCapacityProviders
// ---------------------------------------------------------------------------

pub fn put_cluster_capacity_providers(
    state: &EcsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["cluster"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "cluster is required"))?;

    let name = if cluster_name.starts_with("arn:") {
        cluster_name.split('/').next_back().unwrap_or(cluster_name)
    } else {
        cluster_name
    };

    let mut cluster = state.clusters.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{name}' does not exist"),
        )
    })?;

    let providers: Vec<String> = input["capacityProviders"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let strategy: Vec<Value> = input["defaultCapacityProviderStrategy"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    cluster.capacity_providers = providers;
    cluster.default_capacity_provider_strategy = strategy;

    let cluster_json = json!({
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
    });

    Ok(json!({ "cluster": cluster_json }))
}

// ---------------------------------------------------------------------------
// DescribeCapacityProviders
// ---------------------------------------------------------------------------

pub fn describe_capacity_providers(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let requested: Vec<&str> = input["capacityProviders"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Default system providers always available
    let mut providers: Vec<Value> = vec![
        json!({
            "capacityProviderArn": arn::build_partition(ctx, "ecs", "capacity-provider/FARGATE"),
            "name": "FARGATE",
            "status": "ACTIVE",
            "updateStatus": "DELETE_COMPLETE",
        }),
        json!({
            "capacityProviderArn": arn::build_partition(ctx, "ecs", "capacity-provider/FARGATE_SPOT"),
            "name": "FARGATE_SPOT",
            "status": "ACTIVE",
            "updateStatus": "DELETE_COMPLETE",
        }),
    ];

    // Include any custom capacity providers stored in state
    for entry in state.capacity_providers.iter() {
        let cp = entry.value();
        if requested.is_empty() || requested.contains(&cp.name.as_str()) {
            providers.push(json!({
                "capacityProviderArn": cp.arn,
                "name": cp.name,
                "status": cp.status,
            }));
        }
    }

    // If specific providers were requested, filter
    if !requested.is_empty() {
        providers.retain(|p| requested.contains(&p["name"].as_str().unwrap_or("")));
    }

    Ok(json!({
        "capacityProviders": providers,
        "nextToken": null,
    }))
}

// ---------------------------------------------------------------------------
// PutAccountSetting / ListAccountSettings
// ---------------------------------------------------------------------------

pub fn put_account_setting(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;

    let value = input["value"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "value is required"))?;

    state
        .account_settings
        .insert(name.to_string(), value.to_string());

    Ok(json!({
        "setting": {
            "name": name,
            "value": value,
            "principalArn": format!("arn:{}:iam::000000000000:root", ctx.partition),
        }
    }))
}

pub fn list_account_settings(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_name = input["name"].as_str();
    let effective_settings = input["effectiveSettings"].as_bool().unwrap_or(false);

    // Default settings
    let defaults: HashMap<&str, &str> = [
        ("containerInstanceLongArnFormat", "enabled"),
        ("serviceLongArnFormat", "enabled"),
        ("taskLongArnFormat", "enabled"),
        ("awsvpcTrunking", "disabled"),
        ("containerInsights", "disabled"),
    ]
    .into_iter()
    .collect();

    let settings: Vec<Value> = if effective_settings || state.account_settings.is_empty() {
        // Return defaults merged with stored settings
        defaults
            .iter()
            .filter(|(name, _)| filter_name.is_none_or(|f| **name == f))
            .map(|(name, default_val)| {
                let val = state
                    .account_settings
                    .get(*name)
                    .map(|v| v.clone())
                    .unwrap_or_else(|| default_val.to_string());
                json!({
                    "name": name,
                    "value": val,
                    "principalArn": format!("arn:{}:iam::000000000000:root", ctx.partition),
                })
            })
            .collect()
    } else {
        state
            .account_settings
            .iter()
            .filter(|e| filter_name.is_none_or(|f| e.key() == f))
            .map(|e| {
                json!({
                    "name": e.key(),
                    "value": e.value(),
                    "principalArn": format!("arn:{}:iam::000000000000:root", ctx.partition),
                })
            })
            .collect()
    };

    Ok(json!({ "settings": settings }))
}

// ---------------------------------------------------------------------------
// DiscoverPollEndpoint
// ---------------------------------------------------------------------------

pub fn discover_poll_endpoint(
    _state: &EcsState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "endpoint": format!("http://ecs-agent.{}.amazonaws.com", ctx.region),
        "telemetryEndpoint": format!("http://ecs-telemetry.{}.amazonaws.com", ctx.region),
    }))
}

// ---------------------------------------------------------------------------
// UpdateContainerAgent
// ---------------------------------------------------------------------------

pub fn update_container_agent(
    state: &EcsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["cluster"].as_str().unwrap_or("default");
    let name = if cluster_name.starts_with("arn:") {
        cluster_name.split('/').next_back().unwrap_or(cluster_name)
    } else {
        cluster_name
    };

    // Verify cluster exists (or use default)
    let cluster_exists = state.clusters.contains_key(name);
    if !cluster_exists && name != "default" {
        return Err(AwsError::bad_request(
            "ClusterNotFoundException",
            format!("The specified cluster '{name}' does not exist"),
        ));
    }

    Ok(json!({
        "containerInstance": {
            "containerInstanceArn": format!("arn:{}:ecs:us-east-1:000000000000:container-instance/{}/stub-instance", ctx.partition, name),
            "ec2InstanceId": "i-stub",
            "agentConnected": true,
            "runningTasksCount": 0,
            "pendingTasksCount": 0,
            "status": "ACTIVE",
        }
    }))
}
