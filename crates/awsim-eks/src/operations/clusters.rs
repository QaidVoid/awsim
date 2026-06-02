use awsim_core::{AwsError, RequestContext, arn};
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
    validate_cluster_name(name)?;
    let role_arn = input["roleArn"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "roleArn is required when creating an EKS cluster.",
        )
    })?;
    if role_arn.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "roleArn must not be empty.",
        ));
    }
    if state.clusters.contains_key(name) {
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Cluster {name} already exists"),
        ));
    }
    validate_cluster_logging(&input["logging"])?;

    let arn = arn::build(ctx, "eks", format!("cluster/{name}"));
    let cluster = Cluster {
        name: name.to_string(),
        arn: arn.clone(),
        version: input["version"].as_str().unwrap_or("1.29").to_string(),
        endpoint: format!("https://{name}.eks.{}.amazonaws.com", ctx.region),
        role_arn: role_arn.to_string(),
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
        encryption_config: input["encryptionConfig"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
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
        validate_cluster_logging(l)?;
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

/// Validate `logging.clusterLogging[].types` against the documented
/// EKS control-plane log enum: `api`, `audit`, `authenticator`,
/// `controllerManager`, `scheduler`. AWS rejects unknown types with
/// InvalidParameterException at CreateCluster / UpdateClusterConfig.
fn validate_cluster_logging(value: &Value) -> Result<(), AwsError> {
    if value.is_null() {
        return Ok(());
    }
    let Some(arr) = value.get("clusterLogging").and_then(Value::as_array) else {
        return Ok(());
    };
    for entry in arr {
        let Some(types) = entry.get("types").and_then(Value::as_array) else {
            continue;
        };
        for t in types {
            let s = t.as_str().unwrap_or("");
            if !matches!(
                s,
                "api" | "audit" | "authenticator" | "controllerManager" | "scheduler"
            ) {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    format!(
                        "logging.clusterLogging.types `{s}` must be one of: \
                         api, audit, authenticator, controllerManager, scheduler."
                    ),
                ));
            }
        }
    }
    Ok(())
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
        "encryptionConfig": c.encryption_config,
    })
}

/// `AssociateEncryptionConfig` replaces the cluster's encryptionConfig
/// wholesale. AWS Smithy declares this as a separate API rather than a
/// field on UpdateClusterConfig because the operation runs an
/// asynchronous re-encryption job.
pub fn associate_encryption_config(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
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
    let cfg = input["encryptionConfig"]
        .as_array()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "encryptionConfig is required and must be an array.",
            )
        })?
        .clone();
    if cfg.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "encryptionConfig must contain at least one entry.",
        ));
    }
    c.encryption_config = cfg;
    Ok(json!({
        "update": {
            "id": uuid::Uuid::new_v4().to_string(),
            "status": "InProgress",
            "type": "AssociateEncryptionConfig",
            "params": [],
            "createdAt": now_secs(),
            "errors": [],
        }
    }))
}

/// Validate an EKS cluster name against AWS's documented constraint:
/// 1-100 characters from `[0-9A-Za-z][A-Za-z0-9-_]*`. Real EKS
/// rejects names starting with a hyphen / underscore or containing
/// any other punctuation with InvalidParameterException.
fn validate_cluster_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 100 {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Cluster name length must be between 1 and 100, got {}.",
                name.len()
            ),
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Cluster name '{name}' must start with an ASCII letter or digit."),
        ));
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!(
                    "Cluster name '{name}' contains invalid character '{c}'. \
                     Allowed: alphanumerics, hyphen, underscore."
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod cluster_name_tests {
    use super::*;

    #[test]
    fn accepts_documented_names() {
        validate_cluster_name("prod").unwrap();
        validate_cluster_name("prod-1").unwrap();
        validate_cluster_name("a_b_c").unwrap();
        validate_cluster_name("123abc").unwrap();
    }

    #[test]
    fn rejects_leading_punctuation() {
        assert!(validate_cluster_name("-prod").is_err());
        assert!(validate_cluster_name("_prod").is_err());
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(validate_cluster_name("prod.1").is_err());
        assert!(validate_cluster_name("prod/1").is_err());
        assert!(validate_cluster_name("prod 1").is_err());
    }

    #[test]
    fn rejects_empty_and_too_long() {
        assert!(validate_cluster_name("").is_err());
        let long = "a".repeat(101);
        assert!(validate_cluster_name(&long).is_err());
    }
}

#[cfg(test)]
mod cluster_logging_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("eks", "us-east-1")
    }

    fn base_input() -> Value {
        json!({
            "name": "c1",
            "roleArn": "arn:aws:iam::000000000000:role/eks-cluster",
            "resourcesVpcConfig": { "subnetIds": ["subnet-aaa"] },
        })
    }

    #[test]
    fn accepts_documented_log_types() {
        let state = EksState::default();
        let mut input = base_input();
        input["logging"] = json!({
            "clusterLogging": [
                { "types": ["api", "audit", "controllerManager"], "enabled": true }
            ]
        });
        create_cluster(&state, &input, &ctx()).unwrap();
    }

    #[test]
    fn rejects_unknown_log_type() {
        let state = EksState::default();
        let mut input = base_input();
        input["logging"] = json!({
            "clusterLogging": [
                { "types": ["bogus"], "enabled": true }
            ]
        });
        let err = create_cluster(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn update_cluster_config_validates_logging() {
        let state = EksState::default();
        create_cluster(&state, &base_input(), &ctx()).unwrap();
        let err = update_cluster_config(
            &state,
            &json!({
                "name": "c1",
                "logging": {
                    "clusterLogging": [
                        { "types": ["badtype"], "enabled": true }
                    ]
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }
}
