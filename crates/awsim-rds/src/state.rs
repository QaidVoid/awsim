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
    /// cluster parameter group name → DbClusterParameterGroup
    pub cluster_parameter_groups: DashMap<String, DbClusterParameterGroup>,
    /// ARN → tags
    pub tags: DashMap<String, HashMap<String, String>>,
    /// snapshot identifier → DbSnapshot
    pub snapshots: DashMap<String, DbSnapshot>,
    /// cluster snapshot identifier → DbClusterSnapshot
    pub cluster_snapshots: DashMap<String, DbClusterSnapshot>,
    /// cluster identifier → Vec<DbClusterEndpoint>
    pub cluster_endpoints: DashMap<String, Vec<DbClusterEndpoint>>,
    /// (engine, version) → DbCustomEngineVersion
    pub custom_engine_versions: DashMap<(String, String), DbCustomEngineVersion>,
    /// `GlobalClusterIdentifier` → `DbGlobalCluster`. Aurora global
    /// clusters span regions, so the store lives in every region's
    /// state and the cluster's `members[]` carries each member's
    /// region. AWS clients always reach the global control plane
    /// through the primary region; we don't index by region for that
    /// reason.
    pub global_clusters: DashMap<String, DbGlobalCluster>,
}

/// Serializable snapshot of `RdsState`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RdsStateSnapshot {
    pub instances: Vec<DbInstance>,
    pub clusters: Vec<DbCluster>,
    pub subnet_groups: Vec<DbSubnetGroup>,
    pub parameter_groups: Vec<DbParameterGroup>,
    #[serde(default)]
    pub cluster_parameter_groups: Vec<DbClusterParameterGroup>,
    pub tags: Vec<(String, HashMap<String, String>)>,
    pub snapshots: Vec<DbSnapshot>,
    #[serde(default)]
    pub cluster_snapshots: Vec<DbClusterSnapshot>,
    pub cluster_endpoints: Vec<DbClusterEndpoint>,
    #[serde(default)]
    pub custom_engine_versions: Vec<DbCustomEngineVersion>,
    #[serde(default)]
    pub global_clusters: Vec<DbGlobalCluster>,
}

/// One regional member of an Aurora global cluster. A global cluster
/// has exactly one `primary` member (the writer region) and zero or
/// more `secondary` members (read-only replicas in other regions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbGlobalClusterMember {
    pub db_cluster_arn: String,
    pub region: String,
    /// `primary` or `secondary` — AWS exposes this via the
    /// `IsWriter` boolean on `GlobalClusterMember.IsWriter`.
    pub role: String,
}

/// An Aurora global cluster. AWS exposes it as `arn:aws:rds::<acc>:
/// global-cluster:<id>` (note the empty region segment — global
/// clusters are accountregion-scoped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbGlobalCluster {
    pub identifier: String,
    pub arn: String,
    pub engine: String,
    pub engine_version: String,
    /// `creating` / `available` / `modifying` / `deleting`. AWSim
    /// transitions straight to `available` after `CreateGlobalCluster`
    /// since there is no propagation step to wait on.
    pub status: String,
    pub storage_encrypted: bool,
    pub deletion_protection: bool,
    pub database_name: Option<String>,
    pub members: Vec<DbGlobalClusterMember>,
    pub created_at: String,
}

/// A customer-supplied RDS engine version. AWS allows operators to
/// register a custom build of the supported engines (e.g.
/// `custom-oracle-ee`); AWSim mirrors the surface so SDK clients can
/// round-trip the lifecycle through `pending-validation` ->
/// `available` -> `inactive` even though there is no underlying AMI
/// to actually validate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbCustomEngineVersion {
    pub engine: String,
    pub engine_version: String,
    pub db_engine_version_arn: String,
    /// `pending-validation` -> `available` -> `inactive` (when
    /// `ModifyCustomDBEngineVersion` flips status).
    pub status: String,
    pub description: String,
    pub database_installation_files_s3_bucket_name: Option<String>,
    pub database_installation_files_s3_prefix: Option<String>,
    pub kms_key_id: Option<String>,
    pub created_at: String,
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

/// Aurora Serverless v2 capacity range, measured in Aurora Capacity
/// Units. The cluster scales its writer and readers between these bounds
/// on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerlessV2Scaling {
    pub min_capacity: f64,
    pub max_capacity: f64,
}

/// An IAM role associated with a cluster, granting it access to another
/// AWS service (for example S3 for `aurora_load_from_s3`). AWS reports
/// these under `DescribeDBClusters.AssociatedRoles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbClusterRole {
    pub role_arn: String,
    #[serde(default)]
    pub feature_name: Option<String>,
    pub status: String,
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
    /// Port the cluster's database engine listens on. Defaults to the
    /// engine's standard port at creation.
    #[serde(default)]
    pub port: Option<u16>,
    /// Number of days automated backups are retained. AWS defaults to 1.
    #[serde(default)]
    pub backup_retention_period: Option<u32>,
    /// Daily time range during which automated backups are taken, in the
    /// `hh24:mi-hh24:mi` UTC format.
    #[serde(default)]
    pub preferred_backup_window: Option<String>,
    /// Weekly maintenance window in `ddd:hh24:mi-ddd:hh24:mi` format.
    /// Staged `ModifyDBCluster` changes flush during this window.
    #[serde(default)]
    pub preferred_maintenance_window: Option<String>,
    /// When true, the cluster cannot be deleted until the protection is
    /// removed with `ModifyDBCluster`.
    #[serde(default)]
    pub deletion_protection: bool,
    /// `ModifyDBCluster` with `ApplyImmediately=false` stages the
    /// requested values here until the maintenance window applies them.
    /// AWS surfaces the staged set on `PendingModifiedValues`.
    #[serde(default)]
    pub pending_modified_values: HashMap<String, serde_json::Value>,
    /// `provisioned` or `serverless`. AWS defaults to `provisioned`.
    #[serde(default)]
    pub engine_mode: Option<String>,
    /// Aurora Serverless v2 capacity range, when configured.
    #[serde(default)]
    pub serverless_v2_scaling: Option<ServerlessV2Scaling>,
    /// Whether the RDS Data API HTTP endpoint is enabled for the cluster.
    #[serde(default)]
    pub http_endpoint_enabled: bool,
    /// IAM roles associated with the cluster via `AddRoleToDBCluster`.
    #[serde(default)]
    pub associated_roles: Vec<DbClusterRole>,
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

/// A parameter group applied at the Aurora cluster level. Distinct from
/// the instance-level `DbParameterGroup`: cluster parameter groups
/// configure engine settings shared across every instance in a cluster
/// (for example replication and audit settings) and live in their own
/// `cluster-pg` ARN namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbClusterParameterGroup {
    pub name: String,
    pub arn: String,
    pub family: String,
    pub description: String,
    /// Caller-overridden parameter values keyed by parameter name.
    /// Defaults come from the family catalog and are only stored here
    /// once a caller modifies them.
    #[serde(default)]
    pub parameters: HashMap<String, String>,
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
    /// Master username carried over from the source instance, so a
    /// restore can rebuild the instance with its original credentials.
    #[serde(default)]
    pub master_username: String,
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

/// A point-in-time snapshot of an Aurora DB cluster. Cluster snapshots
/// are distinct from instance snapshots: they capture the cluster's
/// shared storage volume rather than a single instance, and AWS exposes
/// them through a separate `cluster-snapshot` ARN namespace and the
/// `DBClusterSnapshot*` operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbClusterSnapshot {
    pub snapshot_identifier: String,
    pub arn: String,
    pub cluster_identifier: String,
    pub engine: String,
    pub engine_version: String,
    pub master_username: String,
    pub status: String,
    pub created_at: String,
    /// `manual` for user-requested snapshots; `automated` is reserved
    /// for the backup-retention snapshots AWS takes on a schedule.
    pub snapshot_type: String,
    /// KmsKeyId carried over from the source cluster or supplied on copy.
    #[serde(default)]
    pub kms_key_id: Option<String>,
    /// Tags copied from the source cluster plus any supplied to the
    /// create or copy request.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Source region recorded for cross-region copy bookkeeping.
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
