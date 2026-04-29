use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct EfsState {
    /// fs_id → FileSystem
    pub file_systems: DashMap<String, FileSystem>,
    /// mt_id → MountTarget
    pub mount_targets: DashMap<String, MountTarget>,
    /// ap_id → AccessPoint
    pub access_points: DashMap<String, AccessPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystem {
    pub file_system_id: String,
    pub file_system_arn: String,
    pub creation_token: String,
    pub creation_time: f64,
    pub life_cycle_state: String,
    pub number_of_mount_targets: u32,
    pub size_in_bytes_value: u64,
    pub performance_mode: String,
    pub throughput_mode: String,
    pub provisioned_throughput_in_mibps: Option<f64>,
    pub encrypted: bool,
    pub kms_key_id: Option<String>,
    pub name: Option<String>,
    pub tags: HashMap<String, String>,
    pub lifecycle_policies: Vec<serde_json::Value>,
    pub backup_policy_status: String,
    pub file_system_protection_replication_overwrite_protection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountTarget {
    pub mount_target_id: String,
    pub file_system_id: String,
    pub subnet_id: String,
    pub life_cycle_state: String,
    pub ip_address: String,
    pub network_interface_id: String,
    pub availability_zone_id: String,
    pub availability_zone_name: String,
    pub vpc_id: String,
    pub security_groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPoint {
    pub access_point_id: String,
    pub access_point_arn: String,
    pub client_token: String,
    pub file_system_id: String,
    pub posix_user: Option<serde_json::Value>,
    pub root_directory: Option<serde_json::Value>,
    pub life_cycle_state: String,
    pub name: Option<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EfsStateSnapshot {
    pub file_systems: Vec<FileSystem>,
    pub mount_targets: Vec<MountTarget>,
    pub access_points: Vec<AccessPoint>,
}

impl EfsState {
    pub fn to_snapshot(&self) -> EfsStateSnapshot {
        EfsStateSnapshot {
            file_systems: self
                .file_systems
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            mount_targets: self
                .mount_targets
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            access_points: self
                .access_points
                .iter()
                .map(|e| e.value().clone())
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: EfsStateSnapshot) {
        self.file_systems.clear();
        self.mount_targets.clear();
        self.access_points.clear();
        for fs in snap.file_systems {
            self.file_systems.insert(fs.file_system_id.clone(), fs);
        }
        for mt in snap.mount_targets {
            self.mount_targets.insert(mt.mount_target_id.clone(), mt);
        }
        for ap in snap.access_points {
            self.access_points.insert(ap.access_point_id.clone(), ap);
        }
    }
}
