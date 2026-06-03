use std::time::{Duration, SystemTime};

use awsim_core::lifecycle::LifecycleSm;
use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::{Cluster, ClusterState, EksState, now_secs};

/// Wall-clock a cluster spends in `CREATING` before promoting to
/// `ACTIVE`. Collapsed to zero by `AWSIM_LIFECYCLE_FAST`.
const CLUSTER_CREATE_DELAY: Duration = Duration::from_secs(3);
/// Wall-clock a cluster spends in `DELETING` before tick reaps it.
const CLUSTER_DELETE_DELAY: Duration = Duration::from_secs(3);
/// Wall-clock a cluster spends in `UPDATING` before returning to
/// `ACTIVE`.
const CLUSTER_UPDATE_DELAY: Duration = Duration::from_secs(2);

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
        status: ClusterState::Creating.as_wire().to_string(),
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
        sm: LifecycleSm::new(ClusterState::Creating),
        reap_at: None,
    };
    // Schedule the CREATING -> ACTIVE promotion; tick (or a polling
    // Describe) observes the deadline and flips the wire status.
    cluster.sm.start_transition(
        ClusterState::Creating,
        ClusterState::Active,
        CLUSTER_CREATE_DELAY,
    );
    state.clusters.insert(name.to_string(), cluster);
    let c = state.clusters.get(name).expect("just inserted");
    Ok(json!({ "cluster": serialize_cluster(&c) }))
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
    let mut c = state.clusters.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cluster {name} not found"),
        )
    })?;
    // Flip to DELETING immediately (visible to a polling Describe) and
    // arm the reap deadline; `tick` removes the entry once it elapses.
    // Transition from whatever state the cluster is in (a cluster can
    // be deleted mid-CREATE), then no-op on a repeated DeleteCluster
    // since it's already DELETING.
    let current = c.sm.observe(SystemTime::now()).state;
    if current != ClusterState::Deleting {
        c.sm.start_transition(current, ClusterState::Deleting, Duration::ZERO);
    }
    c.reap_at
        .get_or_insert_with(|| SystemTime::now() + CLUSTER_DELETE_DELAY);
    Ok(json!({ "cluster": serialize_cluster(&c) }))
}

pub fn list_clusters(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = cap_max_results(input["maxResults"].as_i64(), 100, 100);
    let mut clusters: Vec<String> = state.clusters.iter().map(|e| e.key().clone()).collect();
    clusters.sort();
    let page = paginate(clusters, max_results, input["nextToken"].as_str(), |s| {
        s.clone()
    })?;
    let mut resp = json!({ "clusters": page.items });
    if let Some(token) = page.next_token {
        resp["nextToken"] = json!(token);
    }
    Ok(resp)
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
    // Surface an observable UPDATING -> ACTIVE blip so a polling
    // DescribeCluster sees the in-flight config update. Only fires
    // from ACTIVE; a busy cluster is left as-is.
    c.sm.start_transition(ClusterState::Active, ClusterState::Updating, Duration::ZERO);
    c.sm.start_transition(
        ClusterState::Updating,
        ClusterState::Active,
        CLUSTER_UPDATE_DELAY,
    );
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
    // Derive the wire `status` from the live state machine so a
    // polling DescribeCluster observes CREATING -> ACTIVE -> DELETING
    // as the scheduled deadlines elapse. Observing here is what
    // promotes a transient cluster even between tick passes.
    let status = c.sm.observe(SystemTime::now()).state.as_wire();
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
        "status": status,
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
