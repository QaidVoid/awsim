use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{EksState, Nodegroup, now_secs};

pub fn create_nodegroup(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "clusterName is required"))?;
    let name = input["nodegroupName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "nodegroupName is required"))?;
    let arn = format!(
        "arn:aws:eks:{}:{}:nodegroup/{}/{}/{}",
        ctx.region,
        ctx.account_id,
        cluster,
        name,
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let ng = Nodegroup {
        cluster_name: cluster.to_string(),
        name: name.to_string(),
        arn: arn.clone(),
        status: "ACTIVE".to_string(),
        capacity_type: input["capacityType"].as_str().unwrap_or("ON_DEMAND").to_string(),
        scaling_config: input["scalingConfig"].clone(),
        instance_types: input["instanceTypes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["t3.medium".to_string()]),
        subnets: input["subnets"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        ami_type: input["amiType"].as_str().unwrap_or("AL2_x86_64").to_string(),
        node_role: input["nodeRole"].as_str().unwrap_or("").to_string(),
        version: input["version"].as_str().unwrap_or("1.29").to_string(),
        release_version: input["releaseVersion"].as_str().unwrap_or("1.29.0-20240101").to_string(),
        disk_size: input["diskSize"].as_u64().unwrap_or(20) as u32,
        tags: input["tags"]
            .as_object()
            .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default(),
        created_at: now_secs(),
    };
    state
        .nodegroups
        .insert((cluster.to_string(), name.to_string()), ng.clone());
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn describe_nodegroup(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["nodegroupName"].as_str().unwrap_or("");
    let ng = state
        .nodegroups
        .get(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Nodegroup {name} not found")))?;
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn delete_nodegroup(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["nodegroupName"].as_str().unwrap_or("");
    let (_, ng) = state
        .nodegroups
        .remove(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Nodegroup {name} not found")))?;
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn list_nodegroups(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let names: Vec<String> = state
        .nodegroups
        .iter()
        .filter(|e| e.key().0 == cluster)
        .map(|e| e.key().1.clone())
        .collect();
    Ok(json!({ "nodegroups": names }))
}

fn serialize_nodegroup(ng: &Nodegroup) -> Value {
    json!({
        "nodegroupName": ng.name,
        "nodegroupArn": ng.arn,
        "clusterName": ng.cluster_name,
        "version": ng.version,
        "releaseVersion": ng.release_version,
        "createdAt": ng.created_at,
        "modifiedAt": ng.created_at,
        "status": ng.status,
        "capacityType": ng.capacity_type,
        "scalingConfig": ng.scaling_config,
        "instanceTypes": ng.instance_types,
        "subnets": ng.subnets,
        "amiType": ng.ami_type,
        "nodeRole": ng.node_role,
        "diskSize": ng.disk_size,
        "tags": ng.tags,
    })
}
