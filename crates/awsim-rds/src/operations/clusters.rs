use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_cluster_already_exists, db_cluster_not_found, invalid_parameter},
    ids::{
        cluster_arn, cluster_endpoint, cluster_reader_endpoint, default_engine_version, now_iso8601,
    },
    state::{DbCluster, DbGlobalCluster, DbGlobalClusterMember, RdsState},
};

use super::{opt_str, require_str};

fn cluster_to_value(c: &DbCluster) -> Value {
    let mut obj = json!({
        "DBClusterIdentifier": c.identifier,
        "DBClusterArn": c.arn,
        "Engine": c.engine,
        "EngineVersion": c.engine_version,
        "Status": c.status,
        "MasterUsername": c.master_username,
        "Endpoint": c.endpoint,
        "ReaderEndpoint": c.reader_endpoint,
        // AWS Aurora clusters have exactly one writer (the first member,
        // by AddRoleToDBCluster ordering); the rest are read replicas.
        "DBClusterMembers": c.members.iter().enumerate().map(|(i, m)| json!({
            "DBInstanceIdentifier": m,
            "IsClusterWriter": i == 0,
            "DBClusterParameterGroupStatus": "in-sync",
            "PromotionTier": i + 1,
        })).collect::<Vec<_>>(),
        "VpcSecurityGroups": c.vpc_security_groups.iter().map(|sg| json!({
            "VpcSecurityGroupId": sg,
            "Status": "active",
        })).collect::<Vec<_>>(),
        "ClusterCreateTime": c.created_at,
        "ActivityStreamStatus": c.activity_stream_status,
    });
    if let Some(ref name) = c.activity_stream_kinesis_stream_name {
        obj["ActivityStreamKinesisStreamName"] = json!(name);
    }
    if let Some(ref k) = c.activity_stream_kms_key_id {
        obj["ActivityStreamKmsKeyId"] = json!(k);
    }
    if let Some(ref mode) = c.activity_stream_mode {
        obj["ActivityStreamMode"] = json!(mode);
    }
    if c.engine == "aurora-mysql"
        && let Some(window) = c.backtrack_window
    {
        // AWS exposes `BacktrackWindow` (the configured retention) and
        // `LatestBacktrackTime` (the oldest rewind-eligible point). The
        // latest time floors at the cluster's creation date so a freshly
        // created cluster doesn't claim retroactive backtrack coverage.
        obj["BacktrackWindow"] = json!(window);
        obj["LatestBacktrackTime"] = json!(c.created_at);
    }
    obj
}

pub fn create_db_cluster(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;
    let engine = require_str(input, "Engine")?;
    let master_username = require_str(input, "MasterUsername")?;
    let _master_password = require_str(input, "MasterUserPassword")?;

    match engine {
        "aurora" | "aurora-mysql" | "aurora-postgresql" | "mysql" | "postgres" | "docdb"
        | "neptune" => {}
        _ => {
            return Err(invalid_parameter(format!(
                "Unknown engine for cluster: {engine}"
            )));
        }
    }

    if state.clusters.contains_key(identifier) {
        return Err(db_cluster_already_exists(identifier));
    }

    let engine_version = opt_str(input, "EngineVersion")
        .unwrap_or_else(|| default_engine_version(engine))
        .to_string();

    let arn = cluster_arn(&ctx.region, &ctx.account_id, identifier);
    let endpoint = cluster_endpoint(identifier, &ctx.region);
    let reader_endpoint = cluster_reader_endpoint(identifier, &ctx.region);

    let vpc_security_groups: Vec<String> = input["VpcSecurityGroupIds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let backtrack_window = input
        .get("BacktrackWindow")
        .and_then(|v| v.as_u64())
        .filter(|&w| w > 0);
    if backtrack_window.is_some() && engine != "aurora-mysql" {
        return Err(invalid_parameter(
            "BacktrackWindow is only supported on `aurora-mysql` clusters.",
        ));
    }
    if let Some(w) = backtrack_window
        && w > 259_200
    {
        return Err(invalid_parameter(format!(
            "BacktrackWindow `{w}` must not exceed 259200 seconds (72 hours).",
        )));
    }

    let cluster = DbCluster {
        identifier: identifier.to_string(),
        arn: arn.clone(),
        engine: engine.to_string(),
        engine_version,
        status: "available".to_string(),
        master_username: master_username.to_string(),
        endpoint,
        reader_endpoint,
        members: vec![],
        created_at: now_iso8601(),
        vpc_security_groups,
        activity_stream_status: "stopped".to_string(),
        activity_stream_kinesis_stream_name: None,
        activity_stream_kms_key_id: None,
        activity_stream_mode: None,
        backtrack_window,
    };

    let result = cluster_to_value(&cluster);
    state.clusters.insert(identifier.to_string(), cluster);

    Ok(json!({ "DBCluster": result }))
}

pub fn delete_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let cluster = state
        .clusters
        .get(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?
        .clone();

    let result = cluster_to_value(&cluster);
    drop(cluster);
    state.clusters.remove(identifier);

    Ok(json!({ "DBCluster": result }))
}

/// Start a Database Activity Stream against the cluster.
///
/// AWS rejects calls against a cluster that already has the stream
/// running (`InvalidDBClusterStateFault`). The KMS key is required;
/// `Mode` defaults to `async`. AWSim collapses the
/// `starting`/`stopping` transient states and reflects the steady
/// state immediately, so the next describe shows the stream as
/// `started`.
pub fn start_activity_stream(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_arn = require_str(input, "ResourceArn")?;
    let kms_key_id = require_str(input, "KmsKeyId")?;
    let mode = opt_str(input, "Mode").unwrap_or("async").to_string();
    if !matches!(mode.as_str(), "sync" | "async") {
        return Err(invalid_parameter("Mode must be one of `sync` or `async`."));
    }

    let identifier = cluster_identifier_from_arn(cluster_arn)
        .ok_or_else(|| db_cluster_not_found(cluster_arn))?;

    let mut cluster = state
        .clusters
        .get_mut(&identifier)
        .ok_or_else(|| db_cluster_not_found(&identifier))?;
    if cluster.activity_stream_status == "started" || cluster.activity_stream_status == "starting" {
        return Err(AwsError::bad_request(
            "InvalidDBClusterStateFault",
            format!(
                "Cluster `{identifier}` already has an active activity stream \
                 ({}).",
                cluster.activity_stream_status
            ),
        ));
    }

    let kinesis_stream_name = format!("aws-rds-das-cluster-{}", cluster.identifier);
    cluster.activity_stream_status = "started".to_string();
    cluster.activity_stream_kinesis_stream_name = Some(kinesis_stream_name.clone());
    cluster.activity_stream_kms_key_id = Some(kms_key_id.to_string());
    cluster.activity_stream_mode = Some(mode.clone());

    Ok(json!({
        "KmsKeyId": kms_key_id,
        "KinesisStreamName": kinesis_stream_name,
        "Status": cluster.activity_stream_status,
        "Mode": mode,
        "ApplyImmediately": input
            .get("ApplyImmediately")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }))
}

/// Stop a Database Activity Stream and clear its config. Calls
/// against a cluster whose stream is already stopped surface
/// `InvalidDBClusterStateFault` to match AWS.
pub fn stop_activity_stream(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_arn = require_str(input, "ResourceArn")?;
    let identifier = cluster_identifier_from_arn(cluster_arn)
        .ok_or_else(|| db_cluster_not_found(cluster_arn))?;

    let mut cluster = state
        .clusters
        .get_mut(&identifier)
        .ok_or_else(|| db_cluster_not_found(&identifier))?;
    if cluster.activity_stream_status == "stopped" || cluster.activity_stream_status == "stopping" {
        return Err(AwsError::bad_request(
            "InvalidDBClusterStateFault",
            format!(
                "Cluster `{identifier}` does not have an active activity stream \
                 ({}).",
                cluster.activity_stream_status
            ),
        ));
    }

    let kms_key_id = cluster.activity_stream_kms_key_id.clone();
    let kinesis_stream_name = cluster.activity_stream_kinesis_stream_name.clone();
    cluster.activity_stream_status = "stopped".to_string();
    cluster.activity_stream_kinesis_stream_name = None;
    cluster.activity_stream_kms_key_id = None;
    cluster.activity_stream_mode = None;

    Ok(json!({
        "KmsKeyId": kms_key_id,
        "KinesisStreamName": kinesis_stream_name,
        "Status": "stopped",
    }))
}

/// Extract a DB cluster identifier from a `DBCluster` ARN. The ARN
/// format is `arn:aws:rds:<region>:<account>:cluster:<identifier>`.
fn cluster_identifier_from_arn(arn: &str) -> Option<String> {
    let suffix = arn.strip_prefix("arn:")?;
    let parts: Vec<&str> = suffix.split(':').collect();
    if parts.len() < 6 {
        // Allow bare identifiers too — Aurora APIs accept either form.
        return Some(arn.to_string());
    }
    if parts[4] != "cluster" {
        return None;
    }
    Some(parts[5].to_string())
}

fn global_cluster_arn(account: &str, identifier: &str) -> String {
    // AWS global cluster ARNs intentionally omit the region segment:
    // `arn:aws:rds::<account>:global-cluster:<id>`.
    format!("arn:aws:rds::{account}:global-cluster:{identifier}")
}

fn global_cluster_to_value(c: &DbGlobalCluster) -> Value {
    json!({
        "GlobalClusterIdentifier": c.identifier,
        "GlobalClusterArn": c.arn,
        "Engine": c.engine,
        "EngineVersion": c.engine_version,
        "Status": c.status,
        "StorageEncrypted": c.storage_encrypted,
        "DeletionProtection": c.deletion_protection,
        "DatabaseName": c.database_name,
        "ClusterCreateTime": c.created_at,
        "GlobalClusterMembers": c.members.iter().map(|m| json!({
            "DBClusterArn": m.db_cluster_arn,
            "Readers": [],
            "IsWriter": m.role == "primary",
        })).collect::<Vec<_>>(),
    })
}

/// Create a new Aurora global cluster. The caller may pre-attach a
/// source cluster via `SourceDBClusterIdentifier`; that cluster
/// becomes the primary member. Without a source the global cluster
/// exists with no members until `CreateDBCluster --GlobalClusterIdentifier`
/// or `RemoveFromGlobalCluster` shapes the membership.
pub fn create_global_cluster(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "GlobalClusterIdentifier")?;
    if state.global_clusters.contains_key(identifier) {
        return Err(AwsError::bad_request(
            "GlobalClusterAlreadyExistsFault",
            format!("Global cluster `{identifier}` already exists."),
        ));
    }

    let source = opt_str(input, "SourceDBClusterIdentifier").map(String::from);
    let mut members = Vec::new();
    let mut engine: Option<String> = opt_str(input, "Engine").map(String::from);
    let mut engine_version: Option<String> = opt_str(input, "EngineVersion").map(String::from);
    if let Some(ref src) = source {
        let src_cluster = state
            .clusters
            .get(src)
            .ok_or_else(|| db_cluster_not_found(src))?;
        members.push(DbGlobalClusterMember {
            db_cluster_arn: src_cluster.arn.clone(),
            region: ctx.region.clone(),
            role: "primary".to_string(),
        });
        engine = engine.or_else(|| Some(src_cluster.engine.clone()));
        engine_version = engine_version.or_else(|| Some(src_cluster.engine_version.clone()));
    }

    let engine = engine.ok_or_else(|| {
        invalid_parameter("Engine is required when SourceDBClusterIdentifier is not specified.")
    })?;
    if !matches!(
        engine.as_str(),
        "aurora" | "aurora-mysql" | "aurora-postgresql"
    ) {
        return Err(invalid_parameter(format!(
            "GlobalCluster engine `{engine}` must be aurora-mysql or aurora-postgresql."
        )));
    }
    let engine_version =
        engine_version.unwrap_or_else(|| default_engine_version(&engine).to_string());

    let cluster = DbGlobalCluster {
        identifier: identifier.to_string(),
        arn: global_cluster_arn(&ctx.account_id, identifier),
        engine,
        engine_version,
        status: "available".to_string(),
        storage_encrypted: input
            .get("StorageEncrypted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        deletion_protection: input
            .get("DeletionProtection")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        database_name: opt_str(input, "DatabaseName").map(String::from),
        members,
        created_at: now_iso8601(),
    };
    let result = global_cluster_to_value(&cluster);
    state
        .global_clusters
        .insert(identifier.to_string(), cluster);
    Ok(json!({ "GlobalCluster": result }))
}

/// Delete an Aurora global cluster. AWS refuses to delete a global
/// cluster that still has members or has DeletionProtection enabled;
/// we mirror both gates.
pub fn delete_global_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "GlobalClusterIdentifier")?;
    let cluster = state
        .global_clusters
        .get(identifier)
        .ok_or_else(|| {
            AwsError::not_found(
                "GlobalClusterNotFoundFault",
                format!("Global cluster `{identifier}` does not exist."),
            )
        })?
        .clone();
    if cluster.deletion_protection {
        return Err(AwsError::bad_request(
            "InvalidGlobalClusterStateFault",
            format!(
                "Global cluster `{identifier}` has DeletionProtection enabled; \
                 disable it before deleting."
            ),
        ));
    }
    if !cluster.members.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidGlobalClusterStateFault",
            format!(
                "Global cluster `{identifier}` still has {} member cluster(s); \
                 remove them with RemoveFromGlobalCluster first.",
                cluster.members.len()
            ),
        ));
    }
    state.global_clusters.remove(identifier);
    Ok(json!({ "GlobalCluster": global_cluster_to_value(&cluster) }))
}

/// Detach a member cluster from the global cluster. The primary
/// member can be removed but only if it is the last member (matches
/// AWS — removing the writer while secondaries exist leaves an
/// orphaned read-only fleet).
pub fn remove_from_global_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "GlobalClusterIdentifier")?;
    let db_cluster_arn = require_str(input, "DbClusterIdentifier")?;
    let mut cluster = state.global_clusters.get_mut(identifier).ok_or_else(|| {
        AwsError::not_found(
            "GlobalClusterNotFoundFault",
            format!("Global cluster `{identifier}` does not exist."),
        )
    })?;
    let idx = cluster
        .members
        .iter()
        .position(|m| m.db_cluster_arn == db_cluster_arn)
        .ok_or_else(|| {
            AwsError::not_found(
                "GlobalClusterMemberNotFoundFault",
                format!(
                    "Global cluster `{identifier}` has no member matching \
                     `{db_cluster_arn}`."
                ),
            )
        })?;
    if cluster.members[idx].role == "primary" && cluster.members.len() > 1 {
        return Err(AwsError::bad_request(
            "InvalidGlobalClusterStateFault",
            "Cannot remove the primary member while secondary members remain.",
        ));
    }
    cluster.members.remove(idx);
    Ok(json!({ "GlobalCluster": global_cluster_to_value(&cluster) }))
}

pub fn describe_global_clusters(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter = opt_str(input, "GlobalClusterIdentifier");
    let items: Vec<Value> = state
        .global_clusters
        .iter()
        .filter(|e| filter.is_none_or(|f| e.value().identifier == f))
        .map(|e| global_cluster_to_value(e.value()))
        .collect();
    Ok(json!({
        "GlobalClusters": { "GlobalCluster": items },
        "Marker": null,
    }))
}

pub fn describe_db_clusters(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_id = opt_str(input, "DBClusterIdentifier");

    if let Some(id) = filter_id {
        let cluster = state
            .clusters
            .get(id)
            .ok_or_else(|| db_cluster_not_found(id))?;
        let items = vec![cluster_to_value(&cluster)];
        return Ok(json!({
            "DBClusters": { "DBCluster": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .clusters
        .iter()
        .map(|e| cluster_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBClusters": { "DBCluster": items },
        "Marker": null,
    }))
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn seed(state: &RdsState, engine: &str) -> String {
        let resp = create_db_cluster(
            state,
            &json!({
                "DBClusterIdentifier": "prod-cluster",
                "Engine": engine,
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
            }),
            &ctx(),
        )
        .unwrap();
        resp["DBCluster"]["DBClusterArn"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn cluster_describe_carries_default_activity_stream_status() {
        let state = RdsState::default();
        seed(&state, "aurora-mysql");
        let resp = describe_db_clusters(
            &state,
            &json!({ "DBClusterIdentifier": "prod-cluster" }),
            &ctx(),
        )
        .unwrap();
        let cluster = &resp["DBClusters"]["DBCluster"][0];
        assert_eq!(cluster["ActivityStreamStatus"], json!("stopped"));
    }

    #[test]
    fn start_activity_stream_transitions_to_started() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        let resp = start_activity_stream(
            &state,
            &json!({
                "ResourceArn": arn,
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
                "Mode": "async",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Status"], json!("started"));
        let stored = state.clusters.get("prod-cluster").unwrap();
        assert_eq!(stored.activity_stream_status, "started");
        assert_eq!(stored.activity_stream_mode.as_deref(), Some("async"));
        assert!(
            stored.activity_stream_kinesis_stream_name.is_some(),
            "kinesis stream name should be populated"
        );
    }

    #[test]
    fn start_activity_stream_rejects_already_running() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        start_activity_stream(
            &state,
            &json!({
                "ResourceArn": arn.clone(),
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap();
        let err = start_activity_stream(
            &state,
            &json!({
                "ResourceArn": arn,
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidDBClusterStateFault");
    }

    #[test]
    fn stop_activity_stream_transitions_back_to_stopped() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        start_activity_stream(
            &state,
            &json!({
                "ResourceArn": arn.clone(),
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = stop_activity_stream(&state, &json!({ "ResourceArn": arn }), &ctx()).unwrap();
        assert_eq!(resp["Status"], json!("stopped"));
        let stored = state.clusters.get("prod-cluster").unwrap();
        assert!(stored.activity_stream_kinesis_stream_name.is_none());
    }

    #[test]
    fn stop_activity_stream_rejects_when_already_stopped() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        let err = stop_activity_stream(&state, &json!({ "ResourceArn": arn }), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidDBClusterStateFault");
    }

    #[test]
    fn aurora_mysql_backtrack_window_surfaces_in_describe() {
        let state = RdsState::default();
        create_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "bt-cluster",
                "Engine": "aurora-mysql",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
                "BacktrackWindow": 3600u64,
            }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_db_clusters(
            &state,
            &json!({ "DBClusterIdentifier": "bt-cluster" }),
            &ctx(),
        )
        .unwrap();
        let cluster = &resp["DBClusters"]["DBCluster"][0];
        assert_eq!(cluster["BacktrackWindow"], json!(3600));
        assert!(cluster["LatestBacktrackTime"].as_str().is_some());
    }

    #[test]
    fn backtrack_window_rejected_on_non_aurora_mysql_engine() {
        let state = RdsState::default();
        let err = create_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "bt-aurora-pg",
                "Engine": "aurora-postgresql",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
                "BacktrackWindow": 3600u64,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn backtrack_window_rejects_over_72h() {
        let state = RdsState::default();
        let err = create_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "bt-too-big",
                "Engine": "aurora-mysql",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
                "BacktrackWindow": 300_000u64,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn create_global_cluster_with_source_pre_attaches_primary() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        let resp = create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-1",
                "SourceDBClusterIdentifier": "prod-cluster",
            }),
            &ctx(),
        )
        .unwrap();
        let members = resp["GlobalCluster"]["GlobalClusterMembers"]
            .as_array()
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["DBClusterArn"], json!(arn));
        assert_eq!(members[0]["IsWriter"], json!(true));
    }

    #[test]
    fn create_global_cluster_without_source_requires_engine() {
        let state = RdsState::default();
        let err = create_global_cluster(
            &state,
            &json!({ "GlobalClusterIdentifier": "gc-no-engine" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn create_global_cluster_rejects_non_aurora_engine() {
        let state = RdsState::default();
        let err = create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-mysql",
                "Engine": "mysql",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn delete_global_cluster_refuses_when_members_attached() {
        let state = RdsState::default();
        seed(&state, "aurora-mysql");
        create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-2",
                "SourceDBClusterIdentifier": "prod-cluster",
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_global_cluster(
            &state,
            &json!({ "GlobalClusterIdentifier": "gc-2" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidGlobalClusterStateFault");
    }

    #[test]
    fn delete_global_cluster_refuses_when_deletion_protected() {
        let state = RdsState::default();
        create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-protected",
                "Engine": "aurora-mysql",
                "DeletionProtection": true,
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_global_cluster(
            &state,
            &json!({ "GlobalClusterIdentifier": "gc-protected" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidGlobalClusterStateFault");
    }

    #[test]
    fn remove_from_global_cluster_unlinks_member() {
        let state = RdsState::default();
        let arn = seed(&state, "aurora-mysql");
        create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-3",
                "SourceDBClusterIdentifier": "prod-cluster",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = remove_from_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-3",
                "DbClusterIdentifier": arn,
            }),
            &ctx(),
        )
        .unwrap();
        let members = resp["GlobalCluster"]["GlobalClusterMembers"]
            .as_array()
            .unwrap();
        assert!(members.is_empty());
        // Now delete is allowed.
        delete_global_cluster(
            &state,
            &json!({ "GlobalClusterIdentifier": "gc-3" }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn describe_global_clusters_round_trips_members() {
        let state = RdsState::default();
        seed(&state, "aurora-postgresql");
        create_global_cluster(
            &state,
            &json!({
                "GlobalClusterIdentifier": "gc-pg",
                "SourceDBClusterIdentifier": "prod-cluster",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_global_clusters(
            &state,
            &json!({ "GlobalClusterIdentifier": "gc-pg" }),
            &ctx(),
        )
        .unwrap();
        let items = resp["GlobalClusters"]["GlobalCluster"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["Engine"], json!("aurora-postgresql"));
    }

    #[test]
    fn backtrack_window_omitted_on_non_aurora_mysql_describe() {
        let state = RdsState::default();
        seed(&state, "aurora-postgresql");
        let resp = describe_db_clusters(
            &state,
            &json!({ "DBClusterIdentifier": "prod-cluster" }),
            &ctx(),
        )
        .unwrap();
        let cluster = &resp["DBClusters"]["DBCluster"][0];
        assert!(cluster.get("BacktrackWindow").is_none());
    }
}
