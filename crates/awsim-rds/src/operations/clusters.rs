use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_cluster_already_exists, db_cluster_not_found, invalid_parameter},
    ids::{
        cluster_arn, cluster_endpoint, cluster_reader_endpoint, default_engine_version, now_iso8601,
    },
    state::{DbCluster, RdsState},
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
