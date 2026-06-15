use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        db_cluster_already_exists, db_cluster_not_found, db_cluster_role_already_exists,
        db_cluster_role_not_found, db_cluster_snapshot_not_found, invalid_parameter,
    },
    ids::{
        cluster_arn, cluster_endpoint, cluster_reader_endpoint, default_engine_version,
        default_port, now_iso8601,
    },
    state::{
        DbCluster, DbClusterRole, DbGlobalCluster, DbGlobalClusterMember, RdsState,
        ServerlessV2Scaling,
    },
};

use super::{opt_str, opt_u32, require_str};

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
        "DeletionProtection": c.deletion_protection,
    });
    if let Some(port) = c.port {
        obj["Port"] = json!(port);
    }
    if let Some(days) = c.backup_retention_period {
        obj["BackupRetentionPeriod"] = json!(days);
    }
    if let Some(ref w) = c.preferred_backup_window {
        obj["PreferredBackupWindow"] = json!(w);
    }
    if let Some(ref w) = c.preferred_maintenance_window {
        obj["PreferredMaintenanceWindow"] = json!(w);
    }
    if !c.pending_modified_values.is_empty() {
        obj["PendingModifiedValues"] =
            serde_json::to_value(&c.pending_modified_values).unwrap_or_else(|_| json!({}));
    }
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
    obj["EngineMode"] = json!(c.engine_mode.as_deref().unwrap_or("provisioned"));
    obj["HttpEndpointEnabled"] = json!(c.http_endpoint_enabled);
    if let Some(ref scaling) = c.serverless_v2_scaling {
        obj["ServerlessV2ScalingConfiguration"] = json!({
            "MinCapacity": scaling.min_capacity,
            "MaxCapacity": scaling.max_capacity,
        });
    }
    obj["AssociatedRoles"] = json!(
        c.associated_roles
            .iter()
            .map(|r| {
                let mut role = json!({ "RoleArn": r.role_arn, "Status": r.status });
                if let Some(ref f) = r.feature_name {
                    role["FeatureName"] = json!(f);
                }
                role
            })
            .collect::<Vec<_>>()
    );
    obj
}

/// Parse and validate the `ServerlessV2ScalingConfiguration` request
/// member. AWS requires the minimum capacity to be at most the maximum,
/// with both within the supported Aurora Capacity Unit range.
fn parse_serverless_scaling(input: &Value) -> Result<Option<ServerlessV2Scaling>, AwsError> {
    let Some(cfg) = input.get("ServerlessV2ScalingConfiguration") else {
        return Ok(None);
    };
    let min_capacity = cfg.get("MinCapacity").and_then(|v| v.as_f64());
    let max_capacity = cfg.get("MaxCapacity").and_then(|v| v.as_f64());
    let (Some(min_capacity), Some(max_capacity)) = (min_capacity, max_capacity) else {
        return Err(invalid_parameter(
            "ServerlessV2ScalingConfiguration requires numeric MinCapacity and MaxCapacity.",
        ));
    };
    if !(0.5..=256.0).contains(&min_capacity) || !(0.5..=256.0).contains(&max_capacity) {
        return Err(invalid_parameter(
            "Serverless v2 capacity must be between 0.5 and 256 Aurora Capacity Units.",
        ));
    }
    if min_capacity > max_capacity {
        return Err(invalid_parameter(
            "Serverless v2 MinCapacity must not exceed MaxCapacity.",
        ));
    }
    Ok(Some(ServerlessV2Scaling {
        min_capacity,
        max_capacity,
    }))
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

    let arn = cluster_arn(&ctx.partition, &ctx.region, &ctx.account_id, identifier);
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

    let serverless_v2_scaling = parse_serverless_scaling(input)?;

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
        port: Some(opt_u32(input, "Port").map_or_else(|| default_port(engine), |p| p as u16)),
        backup_retention_period: Some(opt_u32(input, "BackupRetentionPeriod").unwrap_or(1)),
        preferred_backup_window: opt_str(input, "PreferredBackupWindow").map(str::to_string),
        preferred_maintenance_window: opt_str(input, "PreferredMaintenanceWindow")
            .map(str::to_string),
        deletion_protection: input
            .get("DeletionProtection")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        pending_modified_values: std::collections::HashMap::new(),
        engine_mode: Some(
            opt_str(input, "EngineMode")
                .unwrap_or("provisioned")
                .to_string(),
        ),
        serverless_v2_scaling,
        http_endpoint_enabled: input
            .get("EnableHttpEndpoint")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        associated_roles: Vec::new(),
    };

    let result = cluster_to_value(&cluster);
    state.clusters.insert(identifier.to_string(), cluster);

    Ok(json!({ "DBCluster": result }))
}

/// `RestoreDBClusterFromSnapshot` rebuilds an Aurora cluster from a
/// cluster snapshot. Engine version and master username are inherited
/// from the snapshot; the caller supplies the new cluster identifier and
/// engine. The restored cluster starts with no members, exactly like a
/// freshly created cluster, so instances are added afterwards.
pub fn restore_db_cluster_from_snapshot(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;
    let snapshot_id = require_str(input, "SnapshotIdentifier")?;
    let engine = require_str(input, "Engine")?;

    let snapshot = state
        .cluster_snapshots
        .get(snapshot_id)
        .ok_or_else(|| db_cluster_snapshot_not_found(snapshot_id))?
        .clone();

    if state.clusters.contains_key(identifier) {
        return Err(db_cluster_already_exists(identifier));
    }

    let engine_version = opt_str(input, "EngineVersion")
        .unwrap_or(snapshot.engine_version.as_str())
        .to_string();

    let vpc_security_groups: Vec<String> = input["VpcSecurityGroupIds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let cluster = DbCluster {
        identifier: identifier.to_string(),
        arn: cluster_arn(&ctx.partition, &ctx.region, &ctx.account_id, identifier),
        engine: engine.to_string(),
        engine_version,
        status: "available".to_string(),
        master_username: snapshot.master_username.clone(),
        endpoint: cluster_endpoint(identifier, &ctx.region),
        reader_endpoint: cluster_reader_endpoint(identifier, &ctx.region),
        members: vec![],
        created_at: now_iso8601(),
        vpc_security_groups,
        activity_stream_status: "stopped".to_string(),
        activity_stream_kinesis_stream_name: None,
        activity_stream_kms_key_id: None,
        activity_stream_mode: None,
        backtrack_window: None,
        port: Some(default_port(engine)),
        backup_retention_period: Some(opt_u32(input, "BackupRetentionPeriod").unwrap_or(1)),
        preferred_backup_window: None,
        preferred_maintenance_window: None,
        deletion_protection: input
            .get("DeletionProtection")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        pending_modified_values: std::collections::HashMap::new(),
        engine_mode: Some(
            opt_str(input, "EngineMode")
                .unwrap_or("provisioned")
                .to_string(),
        ),
        serverless_v2_scaling: parse_serverless_scaling(input)?,
        http_endpoint_enabled: false,
        associated_roles: Vec::new(),
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

    if cluster.deletion_protection {
        return Err(AwsError::bad_request(
            "InvalidParameterCombination",
            format!(
                "Cluster `{identifier}` cannot be deleted because deletion \
                 protection is enabled. Disable it with ModifyDBCluster first."
            ),
        ));
    }

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

/// `ModifyDBCluster` updates a cluster's scalar configuration.
///
/// `DeletionProtection`, `PreferredMaintenanceWindow`, and
/// `VpcSecurityGroupIds` apply immediately. `BackupRetentionPeriod`,
/// `PreferredBackupWindow`, `Port`, and `EngineVersion` follow the
/// `ApplyImmediately` flag: when true they apply now, otherwise they are
/// staged under `PendingModifiedValues` and flushed during the cluster's
/// maintenance window.
pub fn modify_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;

    if let Some(window) = opt_str(input, "PreferredMaintenanceWindow") {
        crate::operations::instances::validate_maintenance_window(window)?;
        cluster.preferred_maintenance_window = Some(window.to_string());
    }
    if let Some(protect) = input.get("DeletionProtection").and_then(|v| v.as_bool()) {
        cluster.deletion_protection = protect;
    }
    if let Some(groups) = input["VpcSecurityGroupIds"].as_array() {
        cluster.vpc_security_groups = groups
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(scaling) = parse_serverless_scaling(input)? {
        cluster.serverless_v2_scaling = Some(scaling);
    }
    if let Some(enabled) = input.get("EnableHttpEndpoint").and_then(|v| v.as_bool()) {
        cluster.http_endpoint_enabled = enabled;
    }

    let apply_immediately = input
        .get("ApplyImmediately")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if apply_immediately {
        if let Some(days) = opt_u32(input, "BackupRetentionPeriod") {
            cluster.backup_retention_period = Some(days);
        }
        if let Some(window) = opt_str(input, "PreferredBackupWindow") {
            cluster.preferred_backup_window = Some(window.to_string());
        }
        if let Some(port) = opt_u32(input, "Port") {
            cluster.port = Some(port as u16);
        }
        if let Some(version) = opt_str(input, "EngineVersion") {
            cluster.engine_version = version.to_string();
        }
        cluster.pending_modified_values.clear();
    } else {
        if let Some(days) = opt_u32(input, "BackupRetentionPeriod") {
            cluster
                .pending_modified_values
                .insert("BackupRetentionPeriod".to_string(), json!(days));
        }
        if let Some(window) = opt_str(input, "PreferredBackupWindow") {
            cluster
                .pending_modified_values
                .insert("PreferredBackupWindow".to_string(), json!(window));
        }
        if let Some(port) = opt_u32(input, "Port") {
            cluster
                .pending_modified_values
                .insert("Port".to_string(), json!(port));
        }
        if let Some(version) = opt_str(input, "EngineVersion") {
            cluster
                .pending_modified_values
                .insert("EngineVersion".to_string(), json!(version));
        }
    }

    let result = cluster_to_value(&cluster);
    Ok(json!({ "DBCluster": result }))
}

/// Flush every staged key in a cluster's `pending_modified_values` back
/// onto its live fields, then clear the map. This is the immediate-apply
/// path that the maintenance window runs when `ApplyImmediately` was
/// false. Pure and wall-clock-free so the tick driver decides when to
/// call it.
pub fn apply_pending_cluster_modified_values(cluster: &mut DbCluster) {
    if cluster.pending_modified_values.is_empty() {
        return;
    }
    if let Some(v) = cluster
        .pending_modified_values
        .get("BackupRetentionPeriod")
        .and_then(|v| v.as_u64())
    {
        cluster.backup_retention_period = Some(v as u32);
    }
    if let Some(v) = cluster
        .pending_modified_values
        .get("PreferredBackupWindow")
        .and_then(|v| v.as_str())
    {
        cluster.preferred_backup_window = Some(v.to_string());
    }
    if let Some(v) = cluster
        .pending_modified_values
        .get("Port")
        .and_then(|v| v.as_u64())
    {
        cluster.port = Some(v as u16);
    }
    if let Some(v) = cluster
        .pending_modified_values
        .get("EngineVersion")
        .and_then(|v| v.as_str())
    {
        cluster.engine_version = v.to_string();
    }
    cluster.pending_modified_values.clear();
}

/// Set the status of every member instance of a cluster. Cluster
/// start/stop cascades to the instances that make up the cluster.
fn set_member_status(state: &RdsState, members: &[String], status: &str) {
    for member in members {
        if let Some(mut inst) = state.instances.get_mut(member) {
            inst.status = status.to_string();
        }
    }
}

/// `StartDBCluster` brings a stopped cluster and its members back to
/// `available`.
pub fn start_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;
    if cluster.status != "stopped" {
        return Err(AwsError::bad_request(
            "InvalidDBClusterStateFault",
            format!(
                "Cluster `{identifier}` must be in `stopped` state to start \
                 (current: `{}`).",
                cluster.status
            ),
        ));
    }
    cluster.status = "available".to_string();
    let members = cluster.members.clone();
    let result = cluster_to_value(&cluster);
    drop(cluster);

    set_member_status(state, &members, "available");
    Ok(json!({ "DBCluster": result }))
}

/// `StopDBCluster` stops a running cluster and its members.
pub fn stop_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;
    if cluster.status != "available" {
        return Err(AwsError::bad_request(
            "InvalidDBClusterStateFault",
            format!(
                "Cluster `{identifier}` must be in `available` state to stop \
                 (current: `{}`).",
                cluster.status
            ),
        ));
    }
    cluster.status = "stopped".to_string();
    let members = cluster.members.clone();
    let result = cluster_to_value(&cluster);
    drop(cluster);

    set_member_status(state, &members, "stopped");
    Ok(json!({ "DBCluster": result }))
}

/// `RebootDBCluster` cycles a cluster. The transient `rebooting` state
/// is collapsed, so the cluster returns to `available` immediately.
pub fn reboot_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let cluster = state
        .clusters
        .get(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;
    if cluster.status != "available" {
        return Err(AwsError::bad_request(
            "InvalidDBClusterStateFault",
            format!(
                "Cluster `{identifier}` must be in `available` state to reboot \
                 (current: `{}`).",
                cluster.status
            ),
        ));
    }
    let result = cluster_to_value(&cluster);
    Ok(json!({ "DBCluster": result }))
}

/// `FailoverDBCluster` promotes a reader to writer.
///
/// With an explicit `TargetDBInstanceIdentifier`, that member is moved to
/// the front of the member list (the writer position). Without one, the
/// next reader is promoted. Promotion is expressed purely through member
/// ordering, since `cluster_to_value` treats the first member as the
/// writer.
pub fn failover_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;

    if let Some(target) = opt_str(input, "TargetDBInstanceIdentifier") {
        let pos = cluster.members.iter().position(|m| m == target);
        match pos {
            Some(idx) => {
                let member = cluster.members.remove(idx);
                cluster.members.insert(0, member);
            }
            None => {
                return Err(invalid_parameter(format!(
                    "Target instance `{target}` is not a member of cluster \
                     `{identifier}`."
                )));
            }
        }
    } else if cluster.members.len() >= 2 {
        let member = cluster.members.remove(1);
        cluster.members.insert(0, member);
    }

    let result = cluster_to_value(&cluster);
    Ok(json!({ "DBCluster": result }))
}

/// `AddRoleToDBCluster` associates an IAM role with the cluster, granting
/// it access to another AWS service. AWS rejects a role that is already
/// attached for the same feature with `DBClusterRoleAlreadyExists`.
pub fn add_role_to_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;
    let role_arn = require_str(input, "RoleArn")?;
    let feature_name = opt_str(input, "FeatureName").map(str::to_string);

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;

    if cluster
        .associated_roles
        .iter()
        .any(|r| r.role_arn == role_arn && r.feature_name == feature_name)
    {
        return Err(db_cluster_role_already_exists(role_arn));
    }

    cluster.associated_roles.push(DbClusterRole {
        role_arn: role_arn.to_string(),
        feature_name,
        status: "ACTIVE".to_string(),
    });

    Ok(json!({}))
}

/// `RemoveRoleFromDBCluster` detaches an IAM role from the cluster. AWS
/// rejects a role that is not attached with `DBClusterRoleNotFound`.
pub fn remove_role_from_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;
    let role_arn = require_str(input, "RoleArn")?;
    let feature_name = opt_str(input, "FeatureName").map(str::to_string);

    let mut cluster = state
        .clusters
        .get_mut(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?;

    let before = cluster.associated_roles.len();
    cluster
        .associated_roles
        .retain(|r| !(r.role_arn == role_arn && r.feature_name == feature_name));
    if cluster.associated_roles.len() == before {
        return Err(db_cluster_role_not_found(role_arn));
    }

    Ok(json!({}))
}

/// `EnableHttpEndpoint` turns on the RDS Data API HTTP endpoint for a
/// cluster, addressed by its ARN.
pub fn enable_http_endpoint(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    set_http_endpoint(state, input, true)
}

/// `DisableHttpEndpoint` turns off the RDS Data API HTTP endpoint.
pub fn disable_http_endpoint(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    set_http_endpoint(state, input, false)
}

fn set_http_endpoint(state: &RdsState, input: &Value, enabled: bool) -> Result<Value, AwsError> {
    let resource_arn = require_str(input, "ResourceArn")?;
    let identifier = cluster_identifier_from_arn(resource_arn)
        .ok_or_else(|| db_cluster_not_found(resource_arn))?;

    let mut cluster = state
        .clusters
        .get_mut(&identifier)
        .ok_or_else(|| db_cluster_not_found(&identifier))?;
    cluster.http_endpoint_enabled = enabled;

    Ok(json!({
        "ResourceArn": resource_arn,
        "HttpEndpointEnabled": enabled,
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

fn global_cluster_arn(partition: &str, account: &str, identifier: &str) -> String {
    // AWS global cluster ARNs intentionally omit the region segment:
    // `arn:aws:rds::<account>:global-cluster:<id>`.
    format!("arn:{partition}:rds::{account}:global-cluster:{identifier}")
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
        arn: global_cluster_arn(&ctx.partition, &ctx.account_id, identifier),
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

    let max_records = cap_max_results(input["MaxRecords"].as_i64(), 100, 100);
    let mut items: Vec<(String, Value)> = state
        .clusters
        .iter()
        .map(|e| (e.key().clone(), cluster_to_value(e.value())))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let page = paginate(items, max_records, opt_str(input, "Marker"), |(k, _)| {
        k.clone()
    })?;
    let db_clusters: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    Ok(json!({
        "DBClusters": { "DBCluster": db_clusters },
        "Marker": page.next_token,
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

#[cfg(test)]
mod cluster_lifecycle_tests {
    use super::*;
    use crate::operations::instances::create_db_instance;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_cluster(state: &RdsState) {
        create_db_cluster(
            state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "Engine": "aurora-postgresql",
                "EngineVersion": "15.4",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn add_member(state: &RdsState, instance_id: &str) {
        create_db_instance(
            state,
            &json!({
                "DBInstanceIdentifier": instance_id,
                "DBInstanceClass": "db.r6g.large",
                "Engine": "aurora-postgresql",
                "DBClusterIdentifier": "aurora-pg",
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn describe(state: &RdsState) -> Value {
        let resp = describe_db_clusters(
            state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        resp["DBClusters"]["DBCluster"][0].clone()
    }

    #[test]
    fn modify_applies_deletion_protection_immediately() {
        let state = RdsState::default();
        create_cluster(&state);
        modify_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg", "DeletionProtection": true }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(describe(&state)["DeletionProtection"], true);
    }

    #[test]
    fn modify_with_apply_immediately_changes_backup_retention() {
        let state = RdsState::default();
        create_cluster(&state);
        modify_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "BackupRetentionPeriod": 7,
                "ApplyImmediately": true,
            }),
            &ctx(),
        )
        .unwrap();
        let cluster = describe(&state);
        assert_eq!(cluster["BackupRetentionPeriod"], 7);
        assert!(cluster.get("PendingModifiedValues").is_none());
    }

    #[test]
    fn modify_without_apply_immediately_stages_pending() {
        let state = RdsState::default();
        create_cluster(&state);
        modify_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "EngineVersion": "16.1",
            }),
            &ctx(),
        )
        .unwrap();
        let cluster = describe(&state);
        assert_eq!(cluster["EngineVersion"], "15.4");
        assert_eq!(cluster["PendingModifiedValues"]["EngineVersion"], "16.1");
    }

    #[test]
    fn pending_flush_applies_staged_values() {
        let state = RdsState::default();
        create_cluster(&state);
        modify_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "EngineVersion": "16.1",
                "Port": 6000,
            }),
            &ctx(),
        )
        .unwrap();
        {
            let mut cluster = state.clusters.get_mut("aurora-pg").unwrap();
            apply_pending_cluster_modified_values(&mut cluster);
        }
        let cluster = describe(&state);
        assert_eq!(cluster["EngineVersion"], "16.1");
        assert_eq!(cluster["Port"], 6000);
        assert!(cluster.get("PendingModifiedValues").is_none());
    }

    #[test]
    fn stop_then_start_cascades_to_members() {
        let state = RdsState::default();
        create_cluster(&state);
        add_member(&state, "aurora-pg-1");

        stop_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(describe(&state)["Status"], "stopped");
        assert_eq!(
            state.instances.get("aurora-pg-1").unwrap().status,
            "stopped"
        );

        start_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(describe(&state)["Status"], "available");
        assert_eq!(
            state.instances.get("aurora-pg-1").unwrap().status,
            "available"
        );
    }

    #[test]
    fn stop_rejects_when_not_available() {
        let state = RdsState::default();
        create_cluster(&state);
        stop_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        let err = stop_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidDBClusterStateFault");
    }

    #[test]
    fn reboot_requires_available_state() {
        let state = RdsState::default();
        create_cluster(&state);
        stop_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        let err = reboot_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidDBClusterStateFault");
    }

    #[test]
    fn failover_to_target_promotes_it_to_writer() {
        let state = RdsState::default();
        create_cluster(&state);
        add_member(&state, "aurora-pg-1");
        add_member(&state, "aurora-pg-2");

        failover_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "TargetDBInstanceIdentifier": "aurora-pg-2",
            }),
            &ctx(),
        )
        .unwrap();
        let members = describe(&state)["DBClusterMembers"].clone();
        assert_eq!(members[0]["DBInstanceIdentifier"], "aurora-pg-2");
        assert_eq!(members[0]["IsClusterWriter"], true);
    }

    #[test]
    fn failover_without_target_promotes_next_reader() {
        let state = RdsState::default();
        create_cluster(&state);
        add_member(&state, "aurora-pg-1");
        add_member(&state, "aurora-pg-2");

        failover_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        let members = describe(&state)["DBClusterMembers"].clone();
        assert_eq!(members[0]["DBInstanceIdentifier"], "aurora-pg-2");
    }

    #[test]
    fn failover_rejects_unknown_target() {
        let state = RdsState::default();
        create_cluster(&state);
        add_member(&state, "aurora-pg-1");
        let err = failover_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "TargetDBInstanceIdentifier": "ghost",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn delete_rejected_when_deletion_protection_enabled() {
        let state = RdsState::default();
        create_cluster(&state);
        modify_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg", "DeletionProtection": true }),
            &ctx(),
        )
        .unwrap();
        let err = delete_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombination");
    }
}

#[cfg(test)]
mod restore_cluster_tests {
    use super::*;
    use crate::operations::cluster_snapshots::create_db_cluster_snapshot;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_and_snapshot(state: &RdsState) {
        create_db_cluster(
            state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "Engine": "aurora-postgresql",
                "EngineVersion": "15.4",
                "MasterUsername": "clusteradmin",
                "MasterUserPassword": "secret123",
            }),
            &ctx(),
        )
        .unwrap();
        create_db_cluster_snapshot(
            state,
            &json!({
                "DBClusterSnapshotIdentifier": "csnap-1",
                "DBClusterIdentifier": "aurora-pg",
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn restore_inherits_snapshot_metadata() {
        let state = RdsState::default();
        create_and_snapshot(&state);

        let resp = restore_db_cluster_from_snapshot(
            &state,
            &json!({
                "DBClusterIdentifier": "restored-cluster",
                "SnapshotIdentifier": "csnap-1",
                "Engine": "aurora-postgresql",
            }),
            &ctx(),
        )
        .unwrap();
        let cluster = &resp["DBCluster"];
        assert_eq!(cluster["DBClusterIdentifier"], "restored-cluster");
        assert_eq!(cluster["Engine"], "aurora-postgresql");
        assert_eq!(cluster["EngineVersion"], "15.4");
        assert_eq!(cluster["MasterUsername"], "clusteradmin");
        assert_eq!(cluster["Status"], "available");
        assert!(cluster["DBClusterMembers"].as_array().unwrap().is_empty());
        assert!(
            cluster["Endpoint"]
                .as_str()
                .unwrap()
                .contains("restored-cluster.cluster")
        );
    }

    #[test]
    fn restore_unknown_snapshot_is_not_found() {
        let state = RdsState::default();
        let err = restore_db_cluster_from_snapshot(
            &state,
            &json!({
                "DBClusterIdentifier": "restored-cluster",
                "SnapshotIdentifier": "ghost",
                "Engine": "aurora-postgresql",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBClusterSnapshotNotFoundFault");
    }

    #[test]
    fn restore_onto_existing_cluster_is_rejected() {
        let state = RdsState::default();
        create_and_snapshot(&state);
        let err = restore_db_cluster_from_snapshot(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "SnapshotIdentifier": "csnap-1",
                "Engine": "aurora-postgresql",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBClusterAlreadyExistsFault");
    }
}

#[cfg(test)]
mod serverless_and_roles_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn describe(state: &RdsState, id: &str) -> Value {
        let resp =
            describe_db_clusters(state, &json!({ "DBClusterIdentifier": id }), &ctx()).unwrap();
        resp["DBClusters"]["DBCluster"][0].clone()
    }

    fn create_serverless(state: &RdsState, id: &str) -> Result<Value, AwsError> {
        create_db_cluster(
            state,
            &json!({
                "DBClusterIdentifier": id,
                "Engine": "aurora-postgresql",
                "EngineVersion": "15.4",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
                "EngineMode": "provisioned",
                "ServerlessV2ScalingConfiguration": { "MinCapacity": 0.5, "MaxCapacity": 16.0 },
                "EnableHttpEndpoint": true,
            }),
            &ctx(),
        )
    }

    #[test]
    fn create_records_scaling_engine_mode_and_http_endpoint() {
        let state = RdsState::default();
        create_serverless(&state, "aurora-sv2").unwrap();
        let cluster = describe(&state, "aurora-sv2");
        assert_eq!(cluster["EngineMode"], "provisioned");
        assert_eq!(cluster["HttpEndpointEnabled"], true);
        assert_eq!(
            cluster["ServerlessV2ScalingConfiguration"]["MinCapacity"],
            0.5
        );
        assert_eq!(
            cluster["ServerlessV2ScalingConfiguration"]["MaxCapacity"],
            16.0
        );
    }

    #[test]
    fn create_rejects_inverted_capacity_range() {
        let state = RdsState::default();
        let err = create_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "bad",
                "Engine": "aurora-postgresql",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
                "ServerlessV2ScalingConfiguration": { "MinCapacity": 8.0, "MaxCapacity": 2.0 },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn modify_updates_scaling_configuration() {
        let state = RdsState::default();
        create_serverless(&state, "aurora-sv2").unwrap();
        modify_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-sv2",
                "ServerlessV2ScalingConfiguration": { "MinCapacity": 1.0, "MaxCapacity": 32.0 },
            }),
            &ctx(),
        )
        .unwrap();
        let cluster = describe(&state, "aurora-sv2");
        assert_eq!(
            cluster["ServerlessV2ScalingConfiguration"]["MaxCapacity"],
            32.0
        );
    }

    #[test]
    fn add_and_remove_role() {
        let state = RdsState::default();
        create_serverless(&state, "aurora-sv2").unwrap();
        let role = "arn:aws:iam::000000000000:role/s3-access";

        add_role_to_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-sv2", "RoleArn": role }),
            &ctx(),
        )
        .unwrap();
        let roles = describe(&state, "aurora-sv2")["AssociatedRoles"].clone();
        assert_eq!(roles.as_array().unwrap().len(), 1);
        assert_eq!(roles[0]["RoleArn"], role);
        assert_eq!(roles[0]["Status"], "ACTIVE");

        remove_role_from_db_cluster(
            &state,
            &json!({ "DBClusterIdentifier": "aurora-sv2", "RoleArn": role }),
            &ctx(),
        )
        .unwrap();
        assert!(
            describe(&state, "aurora-sv2")["AssociatedRoles"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn adding_same_role_twice_is_rejected() {
        let state = RdsState::default();
        create_serverless(&state, "aurora-sv2").unwrap();
        let role = "arn:aws:iam::000000000000:role/s3-access";
        let input = json!({ "DBClusterIdentifier": "aurora-sv2", "RoleArn": role });
        add_role_to_db_cluster(&state, &input, &ctx()).unwrap();
        let err = add_role_to_db_cluster(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "DBClusterRoleAlreadyExists");
    }

    #[test]
    fn removing_absent_role_is_not_found() {
        let state = RdsState::default();
        create_serverless(&state, "aurora-sv2").unwrap();
        let err = remove_role_from_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-sv2",
                "RoleArn": "arn:aws:iam::000000000000:role/ghost",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBClusterRoleNotFound");
    }

    #[test]
    fn enable_then_disable_http_endpoint() {
        let state = RdsState::default();
        let resp = create_db_cluster(
            &state,
            &json!({
                "DBClusterIdentifier": "aurora-pg",
                "Engine": "aurora-postgresql",
                "MasterUsername": "admin",
                "MasterUserPassword": "secret123",
            }),
            &ctx(),
        )
        .unwrap();
        let arn = resp["DBCluster"]["DBClusterArn"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(describe(&state, "aurora-pg")["HttpEndpointEnabled"], false);

        enable_http_endpoint(&state, &json!({ "ResourceArn": arn }), &ctx()).unwrap();
        assert_eq!(describe(&state, "aurora-pg")["HttpEndpointEnabled"], true);

        disable_http_endpoint(&state, &json!({ "ResourceArn": arn }), &ctx()).unwrap();
        assert_eq!(describe(&state, "aurora-pg")["HttpEndpointEnabled"], false);
    }
}
