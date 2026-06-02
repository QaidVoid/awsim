use std::collections::HashMap;

use awsim_core::lifecycle::{LifecycleSm, LifecycleState};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Observable control-plane lifecycle for an EKS cluster. AWS exposes
/// these intermediate states to clients polling `DescribeCluster`;
/// emulators that flip straight to `ACTIVE` mask race conditions in
/// caller code. `Creating`/`Updating`/`Deleting` are transient and
/// promote on tick once their deadline elapses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterState {
    Creating,
    Active,
    Updating,
    Deleting,
    Failed,
}

impl LifecycleState for ClusterState {
    fn is_transient(&self) -> bool {
        matches!(
            self,
            ClusterState::Creating | ClusterState::Updating | ClusterState::Deleting
        )
    }
}

impl ClusterState {
    /// Map to the AWS wire vocabulary surfaced as the `status` string.
    pub fn as_wire(&self) -> &'static str {
        match self {
            ClusterState::Creating => "CREATING",
            ClusterState::Active => "ACTIVE",
            ClusterState::Updating => "UPDATING",
            ClusterState::Deleting => "DELETING",
            ClusterState::Failed => "FAILED",
        }
    }
}

/// Observable lifecycle for an EKS managed nodegroup. Mirrors the
/// cluster vocabulary; `Creating`/`Updating`/`Deleting` are transient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodegroupState {
    Creating,
    Active,
    Updating,
    Deleting,
    Failed,
}

impl LifecycleState for NodegroupState {
    fn is_transient(&self) -> bool {
        matches!(
            self,
            NodegroupState::Creating | NodegroupState::Updating | NodegroupState::Deleting
        )
    }
}

impl NodegroupState {
    /// Map to the AWS wire vocabulary surfaced as the `status` string.
    pub fn as_wire(&self) -> &'static str {
        match self {
            NodegroupState::Creating => "CREATING",
            NodegroupState::Active => "ACTIVE",
            NodegroupState::Updating => "UPDATING",
            NodegroupState::Deleting => "DELETING",
            NodegroupState::Failed => "FAILED",
        }
    }
}

#[derive(Debug, Default)]
pub struct EksState {
    pub clusters: DashMap<String, Cluster>,
    pub nodegroups: DashMap<(String, String), Nodegroup>,
    pub fargate_profiles: DashMap<(String, String), FargateProfile>,
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    /// Cluster-managed addons keyed by `(cluster_name, addon_name)`.
    /// AWS lets clusters opt into managed addons like `vpc-cni`,
    /// `coredns`, or `kube-proxy`; configurationValues + resolveConflicts
    /// control how a CreateAddon / UpdateAddon merges with what already
    /// exists in the kube cluster.
    pub addons: DashMap<(String, String), Addon>,
}

/// EKS managed addon. `resolve_conflicts` controls the create / update
/// merge strategy and is one of `NONE`, `OVERWRITE`, or `PRESERVE`.
/// `configuration_values` is opaque JSON/YAML the caller hands in;
/// AWS doesn't shape-check it, so we don't either — only `serialize`
/// is meaningful.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Addon {
    pub cluster_name: String,
    pub addon_name: String,
    pub addon_arn: String,
    pub addon_version: String,
    pub status: String,
    pub service_account_role_arn: Option<String>,
    pub resolve_conflicts: String,
    pub configuration_values: Option<String>,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    pub modified_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cluster {
    pub name: String,
    pub arn: String,
    pub version: String,
    pub endpoint: String,
    pub role_arn: String,
    pub resources_vpc_config: serde_json::Value,
    pub kubernetes_network_config: serde_json::Value,
    pub logging: serde_json::Value,
    pub identity: serde_json::Value,
    pub status: String,
    pub certificate_authority: serde_json::Value,
    pub platform_version: String,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    /// `[{ resources: ["secrets"], provider: { keyArn } }]`. Persisted
    /// verbatim and replaced wholesale by AssociateEncryptionConfig.
    #[serde(default)]
    pub encryption_config: Vec<serde_json::Value>,
    /// Drives the observable `CREATING -> ACTIVE -> DELETING` lifecycle.
    /// Skipped from (de)serialization: the crate has no snapshot today,
    /// so the SM is reconstructed in `ACTIVE` on the rare deserialize
    /// path via `Default`.
    #[serde(skip, default = "cluster_sm_active")]
    pub sm: LifecycleSm<ClusterState>,
    /// Absolute wall clock at which a `DELETING` cluster is reaped from
    /// the store by `tick`. `None` until DeleteCluster is called.
    #[serde(skip)]
    pub reap_at: Option<std::time::SystemTime>,
}

/// Default cluster SM used when deserializing (no snapshot today):
/// land in `ACTIVE` so a restored cluster is immediately usable.
fn cluster_sm_active() -> LifecycleSm<ClusterState> {
    LifecycleSm::new(ClusterState::Active)
}

// `LifecycleSm` wraps a `Mutex` and isn't `Clone`, so the derive is
// hand-rolled: every plain field is cloned and the SM is rebuilt from
// its current observed state (no pending transition is carried over,
// which is fine since clones are taken only to shape a response body).
impl Clone for Cluster {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            arn: self.arn.clone(),
            version: self.version.clone(),
            endpoint: self.endpoint.clone(),
            role_arn: self.role_arn.clone(),
            resources_vpc_config: self.resources_vpc_config.clone(),
            kubernetes_network_config: self.kubernetes_network_config.clone(),
            logging: self.logging.clone(),
            identity: self.identity.clone(),
            status: self.status.clone(),
            certificate_authority: self.certificate_authority.clone(),
            platform_version: self.platform_version.clone(),
            tags: self.tags.clone(),
            created_at: self.created_at,
            encryption_config: self.encryption_config.clone(),
            sm: LifecycleSm::new(self.sm.current()),
            reap_at: self.reap_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Nodegroup {
    pub cluster_name: String,
    pub name: String,
    pub arn: String,
    pub status: String,
    pub capacity_type: String,
    pub scaling_config: serde_json::Value,
    pub instance_types: Vec<String>,
    pub subnets: Vec<String>,
    pub ami_type: String,
    pub node_role: String,
    pub version: String,
    pub release_version: String,
    pub disk_size: u32,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    /// Free-form k/v labels applied to nodegroup pods. Persisted verbatim;
    /// AWS only constrains label keys/values at the kubelet, not the API.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Node taints. Each entry is `{ key, value, effect }`. `effect`
    /// validated at CreateNodegroup against the kubernetes taint enum.
    #[serde(default)]
    pub taints: Vec<serde_json::Value>,
    /// Optional remoteAccess config: `{ ec2SshKey, sourceSecurityGroups[] }`.
    #[serde(default)]
    pub remote_access: Option<serde_json::Value>,
    /// Optional EC2 LaunchTemplate reference: `{ name OR id, version }`.
    /// AWS requires exactly one of `name`/`id`; both is an error, and
    /// neither is also an error when the launchTemplate block is
    /// present.
    #[serde(default)]
    pub launch_template: Option<serde_json::Value>,
    /// Drives the observable `CREATING -> ACTIVE` lifecycle. Skipped
    /// from (de)serialization for the same reason as `Cluster::sm`.
    #[serde(skip, default = "nodegroup_sm_active")]
    pub sm: LifecycleSm<NodegroupState>,
}

/// Default nodegroup SM used when deserializing (no snapshot today):
/// land in `ACTIVE` so a restored nodegroup is immediately usable.
fn nodegroup_sm_active() -> LifecycleSm<NodegroupState> {
    LifecycleSm::new(NodegroupState::Active)
}

// Hand-rolled for the same reason as `Cluster::clone`: `LifecycleSm`
// isn't `Clone`, so it's rebuilt from its current observed state.
impl Clone for Nodegroup {
    fn clone(&self) -> Self {
        Self {
            cluster_name: self.cluster_name.clone(),
            name: self.name.clone(),
            arn: self.arn.clone(),
            status: self.status.clone(),
            capacity_type: self.capacity_type.clone(),
            scaling_config: self.scaling_config.clone(),
            instance_types: self.instance_types.clone(),
            subnets: self.subnets.clone(),
            ami_type: self.ami_type.clone(),
            node_role: self.node_role.clone(),
            version: self.version.clone(),
            release_version: self.release_version.clone(),
            disk_size: self.disk_size,
            tags: self.tags.clone(),
            created_at: self.created_at,
            labels: self.labels.clone(),
            taints: self.taints.clone(),
            remote_access: self.remote_access.clone(),
            launch_template: self.launch_template.clone(),
            sm: LifecycleSm::new(self.sm.current()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FargateProfile {
    pub cluster_name: String,
    pub name: String,
    pub arn: String,
    pub pod_execution_role_arn: String,
    pub subnets: Vec<String>,
    pub selectors: Vec<serde_json::Value>,
    pub status: String,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
