use std::collections::HashMap;
use std::sync::Mutex;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct MemoryDbState {
    pub clusters: DashMap<String, Cluster>,
    pub users: DashMap<String, User>,
    pub acls: DashMap<String, Acl>,
    pub snapshots: DashMap<String, Snapshot>,
    pub subnet_groups: DashMap<String, SubnetGroup>,
    pub parameter_groups: DashMap<String, ParameterGroup>,
    /// Tags keyed by resource ARN. AWS MemoryDB allows tagging any
    /// resource (cluster, user, ACL, snapshot, subnet group, parameter
    /// group) through the same TagResource API; the per-ARN map keeps
    /// lookup O(1) without coupling tags to each resource struct.
    pub tags: DashMap<String, HashMap<String, String>>,
    /// Ring of recent control-plane events emitted by mutating ops.
    /// Capped at [`MAX_EVENTS`] so a long-running process does not
    /// grow without bound; oldest entries are evicted first.
    pub events: Mutex<Vec<Event>>,
}

/// Maximum number of MemoryDB events retained per account+region. AWS
/// stores events for 14 days; the emulator caps by count to keep
/// snapshot sizes bounded.
pub const MAX_EVENTS: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Epoch seconds when the event was recorded.
    pub date: u64,
    pub source_name: String,
    /// `cluster | parameter-group | subnet-group | user | acl | snapshot`.
    pub source_type: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub node_type: String,
    /// Cluster engine. AWS supports `redis` and `valkey`. Defaults to
    /// `redis` on legacy snapshots created before this field existed.
    #[serde(default = "default_engine")]
    pub engine: String,
    pub engine_version: String,
    pub engine_patch_version: String,
    pub parameter_group_name: String,
    pub parameter_group_status: String,
    pub subnet_group_name: String,
    pub security_group_ids: Vec<String>,
    pub acl_name: String,
    pub auto_minor_version_upgrade: bool,
    pub cluster_endpoint: serde_json::Value,
    pub number_of_shards: u32,
    /// Replica count per shard. Combined with [`number_of_shards`] this
    /// dictates how many `Nodes` entries appear under each shard.
    /// Defaults to zero (primary only) on legacy snapshots.
    #[serde(default)]
    pub num_replicas_per_shard: u32,
    pub tls_enabled: bool,
    pub kms_key_id: Option<String>,
    pub maintenance_window: String,
    pub snapshot_retention_limit: u32,
    pub snapshot_window: String,
    pub sns_topic_arn: Option<String>,
    pub sns_topic_status: String,
    pub description: Option<String>,
    /// MemoryDB data tiering: when true the cluster spills cold keys
    /// to NVMe, which AWS only supports on `db.r6gd.*` node types.
    #[serde(default)]
    pub data_tiering: bool,
    /// Cluster network stack. One of `ipv4`, `ipv6`, or `dual_stack`.
    /// Defaults to `ipv4` when unset on legacy snapshots.
    #[serde(default = "default_network_type")]
    pub network_type: String,
    /// IP discovery protocol exposed to clients. One of `ipv4` or
    /// `ipv6`. Defaults to `ipv4` when unset on legacy snapshots.
    #[serde(default = "default_ip_discovery")]
    pub ip_discovery: String,
    /// Snapshot ARNs supplied to seed the cluster at restore time.
    /// Captured for audit/echo; the emulator does not load object
    /// data from the ARNs.
    #[serde(default)]
    pub snapshot_arns: Vec<String>,
    /// Snapshot name supplied to seed the cluster at restore time.
    /// `None` when the cluster was created from scratch.
    #[serde(default)]
    pub snapshot_name: Option<String>,
    /// Name of the multi-region cluster this cluster belongs to, if
    /// any. Multi-region clusters omit the region from their ARN per
    /// AWS docs.
    #[serde(default)]
    pub multi_region_cluster_name: Option<String>,
}

fn default_engine() -> String {
    "redis".to_string()
}

fn default_network_type() -> String {
    "ipv4".to_string()
}

fn default_ip_discovery() -> String {
    "ipv4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub access_string: String,
    pub minimum_engine_version: String,
    pub authentication_mode: String,
    /// Number of passwords associated with the user. Always zero for
    /// `iam` / `no-password-required` authentication types.
    #[serde(default)]
    pub password_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acl {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub user_names: Vec<String>,
    pub minimum_engine_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    pub arn: String,
    pub status: String,
    pub source: String,
    pub kms_key_id: Option<String>,
    pub cluster_name: String,
    /// Frozen copy of the source cluster's topology at snapshot time,
    /// returned to clients verbatim as `ClusterConfiguration`. Legacy
    /// snapshots without this field default to a minimal `{Name}` shape.
    #[serde(default = "default_cluster_config")]
    pub cluster_config: serde_json::Value,
}

fn default_cluster_config() -> serde_json::Value {
    serde_json::Value::Null
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetGroup {
    pub name: String,
    pub arn: String,
    pub description: Option<String>,
    pub vpc_id: String,
    pub subnet_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGroup {
    pub name: String,
    pub arn: String,
    pub family: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryDbSnapshot {
    pub clusters: Vec<Cluster>,
    pub users: Vec<User>,
    pub acls: Vec<Acl>,
    pub snapshots: Vec<Snapshot>,
    pub subnet_groups: Vec<SubnetGroup>,
    pub parameter_groups: Vec<ParameterGroup>,
    #[serde(default)]
    pub tags: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl MemoryDbState {
    pub fn to_snapshot(&self) -> MemoryDbSnapshot {
        MemoryDbSnapshot {
            clusters: self.clusters.iter().map(|e| e.value().clone()).collect(),
            users: self.users.iter().map(|e| e.value().clone()).collect(),
            acls: self.acls.iter().map(|e| e.value().clone()).collect(),
            snapshots: self.snapshots.iter().map(|e| e.value().clone()).collect(),
            subnet_groups: self
                .subnet_groups
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            parameter_groups: self
                .parameter_groups
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            tags: self
                .tags
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
            events: self
                .events
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: MemoryDbSnapshot) {
        self.clusters.clear();
        self.users.clear();
        self.acls.clear();
        self.snapshots.clear();
        self.subnet_groups.clear();
        self.parameter_groups.clear();
        self.tags.clear();
        for (arn, tags) in snap.tags {
            self.tags.insert(arn, tags);
        }
        if let Ok(mut guard) = self.events.lock() {
            guard.clear();
            guard.extend(snap.events);
        }
        for c in snap.clusters {
            self.clusters.insert(c.name.clone(), c);
        }
        for u in snap.users {
            self.users.insert(u.name.clone(), u);
        }
        for a in snap.acls {
            self.acls.insert(a.name.clone(), a);
        }
        for s in snap.snapshots {
            self.snapshots.insert(s.name.clone(), s);
        }
        for sg in snap.subnet_groups {
            self.subnet_groups.insert(sg.name.clone(), sg);
        }
        for pg in snap.parameter_groups {
            self.parameter_groups.insert(pg.name.clone(), pg);
        }
    }

    /// Append an event to the ring buffer. Drops the oldest entry
    /// once [`MAX_EVENTS`] is exceeded to keep the buffer bounded.
    pub fn push_event(&self, source_type: &str, source_name: &str, message: impl Into<String>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let evt = Event {
            date: now,
            source_name: source_name.to_string(),
            source_type: source_type.to_string(),
            message: message.into(),
        };
        if let Ok(mut guard) = self.events.lock() {
            if guard.len() >= MAX_EVENTS {
                guard.remove(0);
            }
            guard.push(evt);
        }
    }
}
