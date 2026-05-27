use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// RDS state — per account+region.
#[derive(Debug, Default)]
pub struct RdsState {
    pub instances: DashMap<String, DbInstance>,
    pub clusters: DashMap<String, DbCluster>,
    pub subnet_groups: DashMap<String, DbSubnetGroup>,
    pub parameter_groups: DashMap<String, DbParameterGroup>,
    /// ARN → tags
    pub tags: DashMap<String, HashMap<String, String>>,
    /// snapshot identifier → DbSnapshot
    pub snapshots: DashMap<String, DbSnapshot>,
    /// cluster identifier → Vec<DbClusterEndpoint>
    pub cluster_endpoints: DashMap<String, Vec<DbClusterEndpoint>>,
}

/// Serializable snapshot of `RdsState`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RdsStateSnapshot {
    pub instances: Vec<DbInstance>,
    pub clusters: Vec<DbCluster>,
    pub subnet_groups: Vec<DbSubnetGroup>,
    pub parameter_groups: Vec<DbParameterGroup>,
    pub tags: Vec<(String, HashMap<String, String>)>,
    pub snapshots: Vec<DbSnapshot>,
    pub cluster_endpoints: Vec<DbClusterEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbInstance {
    pub identifier: String,
    pub arn: String,
    pub instance_class: String,
    pub engine: String,
    pub engine_version: String,
    /// available, creating, deleting, stopped, starting, stopping, rebooting
    pub status: String,
    pub master_username: String,
    pub allocated_storage: u32,
    pub endpoint: Option<DbEndpoint>,
    pub subnet_group_name: Option<String>,
    pub vpc_security_groups: Vec<String>,
    pub multi_az: bool,
    pub publicly_accessible: bool,
    pub storage_type: String,
    pub cluster_identifier: Option<String>,
    pub created_at: String,
    /// Provisioned IOPS. Only meaningful for `io1`/`io2`/`gp3`; AWS
    /// rejects any non-zero Iops on `gp2`/`magnetic`.
    #[serde(default)]
    pub iops: Option<u32>,
    /// Provisioned storage throughput in MiB/s. Only valid on `gp3`.
    #[serde(default)]
    pub storage_throughput: Option<u32>,
    /// AWS license model — one of `general-public-license`,
    /// `license-included`, `bring-your-own-license`. Allowed values
    /// depend on the engine.
    #[serde(default)]
    pub license_model: Option<String>,
    /// When true, CreateDBSnapshot copies the instance's tags onto the
    /// new snapshot. AWS defaults to false.
    #[serde(default)]
    pub copy_tags_to_snapshot: bool,
    /// KmsKeyId used at instance creation; propagated onto snapshots
    /// taken from this instance.
    #[serde(default)]
    pub kms_key_id: Option<String>,
    /// Enhanced monitoring interval in seconds. 0 disables it; AWS
    /// accepts 0, 1, 5, 10, 15, 30, 60.
    #[serde(default)]
    pub monitoring_interval: Option<u32>,
    /// IAM role used by enhanced monitoring to publish metrics. Required
    /// by AWS whenever monitoring_interval > 0.
    #[serde(default)]
    pub monitoring_role_arn: Option<String>,
    /// Log types enabled for export to CloudWatch Logs. Engine-specific:
    /// e.g. ["error","slowquery"] for MySQL or ["postgresql"] for PG.
    #[serde(default)]
    pub enabled_cloudwatch_logs_exports: Vec<String>,
    /// Weekly maintenance window in the AWS-documented format
    /// `ddd:hh24:mi-ddd:hh24:mi`. AWS assigns a default (30-minute
    /// window in the region's "off-hours" block) if the caller does
    /// not specify one; we mirror that by stamping `sun:05:00-sun:05:30`
    /// when omitted.
    #[serde(default)]
    pub preferred_maintenance_window: Option<String>,
    /// `ModifyDBInstance` with `ApplyImmediately=false` (the default for
    /// destructive changes) stages the diff here until the next
    /// maintenance window applies it. AWS returns the staged set on
    /// `DescribeDBInstances.PendingModifiedValues`; an empty map means
    /// no changes are pending.
    #[serde(default)]
    pub pending_modified_values: HashMap<String, serde_json::Value>,
    /// Identifier of the source instance when this row is a read
    /// replica. AWS exposes it as `ReadReplicaSourceDBInstanceIdentifier`
    /// on describe.
    #[serde(default)]
    pub read_replica_source_db_instance_identifier: Option<String>,
    /// Identifiers of read replicas pointed at this instance. AWS
    /// surfaces this on the source so callers can fan out a delete
    /// across the replica tree.
    #[serde(default)]
    pub read_replica_db_instance_identifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbEndpoint {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbCluster {
    pub identifier: String,
    pub arn: String,
    pub engine: String,
    pub engine_version: String,
    pub status: String,
    pub master_username: String,
    pub endpoint: String,
    pub reader_endpoint: String,
    /// instance identifiers
    pub members: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub vpc_security_groups: Vec<String>,
    /// Database Activity Stream status. AWS exposes the four-state
    /// machine on `DescribeDBClusters.ActivityStreamStatus`:
    /// `stopped` -> `starting` -> `started` -> `stopping` ->
    /// `stopped`. We collapse the transient states for the synthetic
    /// case (no real Kinesis consumer to wait on) and leap straight
    /// to the steady-state value.
    #[serde(default = "default_activity_stream_status")]
    pub activity_stream_status: String,
    /// Optional Kinesis stream that buffers Activity Stream events.
    /// AWS picks the name when the activity stream starts; we
    /// derive it from the cluster identifier.
    #[serde(default)]
    pub activity_stream_kinesis_stream_name: Option<String>,
    /// Activity Stream KMS key configured by the caller on
    /// `StartActivityStream`.
    #[serde(default)]
    pub activity_stream_kms_key_id: Option<String>,
    /// `sync` or `async`. AWS defaults to `async` when omitted.
    #[serde(default)]
    pub activity_stream_mode: Option<String>,
    /// Aurora MySQL clusters surface `BacktrackWindow` (seconds
    /// AWS retains for rewind) and `LatestBacktrackTime` (the
    /// oldest point currently backtrack-eligible). We persist the
    /// configured window; the latest time is derived from the
    /// configured retention and cluster age on every describe.
    #[serde(default)]
    pub backtrack_window: Option<u64>,
}

fn default_activity_stream_status() -> String {
    "stopped".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSubnetGroup {
    pub name: String,
    pub arn: String,
    pub description: String,
    pub subnet_ids: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbParameterGroup {
    pub name: String,
    pub arn: String,
    pub family: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSnapshot {
    pub snapshot_identifier: String,
    pub arn: String,
    pub db_instance_identifier: String,
    pub engine: String,
    pub engine_version: String,
    pub allocated_storage: u32,
    pub status: String,
    pub created_at: String,
    /// Tags copied from the source DB instance when
    /// `CopyTagsToSnapshot=true`, plus any tags supplied directly to
    /// CreateDBSnapshot.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// KmsKeyId carried over from the source DB instance when set.
    #[serde(default)]
    pub kms_key_id: Option<String>,
    /// Source region for cross-region copy bookkeeping.
    #[serde(default)]
    pub source_region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbClusterEndpoint {
    pub endpoint_identifier: String,
    pub arn: String,
    pub cluster_identifier: String,
    pub endpoint_type: String,
    pub endpoint: String,
    pub status: String,
    pub custom_endpoint_type: Option<String>,
}
