use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::{EksState, FargateProfile, now_secs};

pub fn create_fargate_profile(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required")
    })?;
    let name = input["fargateProfileName"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "fargateProfileName is required",
        )
    })?;
    let arn = arn::build(
        ctx,
        "eks",
        format!(
            "fargateprofile/{}/{}/{}",
            cluster,
            name,
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ),
    );
    let fp = FargateProfile {
        cluster_name: cluster.to_string(),
        name: name.to_string(),
        arn: arn.clone(),
        pod_execution_role_arn: input["podExecutionRoleArn"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        subnets: input["subnets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        selectors: input["selectors"].as_array().cloned().unwrap_or_default(),
        status: "ACTIVE".to_string(),
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
    state
        .fargate_profiles
        .insert((cluster.to_string(), name.to_string()), fp.clone());
    Ok(json!({ "fargateProfile": serialize_fp(&fp) }))
}

pub fn describe_fargate_profile(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["fargateProfileName"].as_str().unwrap_or("");
    let fp = state
        .fargate_profiles
        .get(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("FargateProfile {name} not found"),
            )
        })?;
    Ok(json!({ "fargateProfile": serialize_fp(&fp) }))
}

pub fn delete_fargate_profile(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["fargateProfileName"].as_str().unwrap_or("");
    let (_, fp) = state
        .fargate_profiles
        .remove(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("FargateProfile {name} not found"),
            )
        })?;
    Ok(json!({ "fargateProfile": serialize_fp(&fp) }))
}

pub fn list_fargate_profiles(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let names: Vec<String> = state
        .fargate_profiles
        .iter()
        .filter(|e| e.key().0 == cluster)
        .map(|e| e.key().1.clone())
        .collect();
    Ok(json!({ "fargateProfileNames": names }))
}

fn serialize_fp(fp: &FargateProfile) -> Value {
    json!({
        "fargateProfileName": fp.name,
        "fargateProfileArn": fp.arn,
        "clusterName": fp.cluster_name,
        "createdAt": fp.created_at,
        "podExecutionRoleArn": fp.pod_execution_role_arn,
        "subnets": fp.subnets,
        "selectors": fp.selectors,
        "status": fp.status,
        "tags": fp.tags,
    })
}
