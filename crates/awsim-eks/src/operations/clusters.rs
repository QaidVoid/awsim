use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Cluster, EksState, now_secs};

pub fn create_cluster(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;
    if state.clusters.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Cluster {name} already exists"),
        ));
    }
    let arn = format!(
        "arn:aws:eks:{}:{}:cluster/{}",
        ctx.region, ctx.account_id, name
    );
    let cluster = Cluster {
        name: name.to_string(),
        arn: arn.clone(),
        version: input["version"].as_str().unwrap_or("1.29").to_string(),
        endpoint: format!("https://{name}.eks.{}.amazonaws.com", ctx.region),
        role_arn: input["roleArn"].as_str().unwrap_or("").to_string(),
        resources_vpc_config: input["resourcesVpcConfig"].clone(),
        kubernetes_network_config: input["kubernetesNetworkConfig"].clone(),
        logging: input["logging"].clone(),
        identity: json!({ "oidc": { "issuer": format!("https://oidc.eks.{}.amazonaws.com/id/EXAMPLED539D4633E53DE1B716D3041E", ctx.region) } }),
        status: "ACTIVE".to_string(),
        certificate_authority: json!({ "data": "LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0t" }),
        platform_version: "eks.1".to_string(),
        tags: input["tags"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: now_secs(),
    };
    state.clusters.insert(name.to_string(), cluster.clone());
    Ok(json!({ "cluster": serialize_cluster(&cluster) }))
}

pub fn describe_cluster(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;
    let c = state.clusters.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cluster {name} not found"),
        )
    })?;
    Ok(json!({ "cluster": serialize_cluster(&c) }))
}

pub fn delete_cluster(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;
    let (_, c) = state.clusters.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cluster {name} not found"),
        )
    })?;
    Ok(json!({ "cluster": serialize_cluster(&c) }))
}

pub fn list_clusters(
    state: &EksState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let clusters: Vec<String> = state.clusters.iter().map(|e| e.key().clone()).collect();
    Ok(json!({ "clusters": clusters }))
}

pub fn update_cluster_config(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "name is required"))?;
    let mut c = state.clusters.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cluster {name} not found"),
        )
    })?;
    if let Some(l) = input.get("logging") {
        c.logging = l.clone();
    }
    if let Some(v) = input.get("resourcesVpcConfig") {
        c.resources_vpc_config = v.clone();
    }
    Ok(json!({
        "update": {
            "id": uuid::Uuid::new_v4().to_string(),
            "status": "InProgress",
            "type": "ConfigUpdate",
            "params": [],
            "createdAt": now_secs(),
            "errors": [],
        },
        "_region": ctx.region,
    }))
}

pub(crate) fn serialize_cluster(c: &Cluster) -> Value {
    json!({
        "name": c.name,
        "arn": c.arn,
        "createdAt": c.created_at,
        "version": c.version,
        "endpoint": c.endpoint,
        "roleArn": c.role_arn,
        "resourcesVpcConfig": c.resources_vpc_config,
        "kubernetesNetworkConfig": c.kubernetes_network_config,
        "logging": c.logging,
        "identity": c.identity,
        "status": c.status,
        "certificateAuthority": c.certificate_authority,
        "platformVersion": c.platform_version,
        "tags": c.tags,
    })
}
