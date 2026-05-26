use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct EksState {
    pub clusters: DashMap<String, Cluster>,
    pub nodegroups: DashMap<(String, String), Nodegroup>,
    pub fargate_profiles: DashMap<(String, String), FargateProfile>,
    pub resource_tags: DashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
