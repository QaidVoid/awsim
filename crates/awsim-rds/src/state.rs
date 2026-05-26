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
