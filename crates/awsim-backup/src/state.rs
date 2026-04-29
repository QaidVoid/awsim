use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct BackupState {
    pub vaults: DashMap<String, BackupVault>,
    pub plans: DashMap<String, BackupPlan>,
    /// (plan_id, selection_id) keyed selections.
    pub selections: DashMap<String, BackupSelection>,
    pub jobs: DashMap<String, BackupJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupVault {
    pub name: String,
    pub arn: String,
    pub creation_date: f64,
    pub encryption_key_arn: Option<String>,
    pub creator_request_id: Option<String>,
    pub number_of_recovery_points: u32,
    pub locked: bool,
    pub min_retention_days: Option<u32>,
    pub max_retention_days: Option<u32>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPlan {
    pub plan_id: String,
    pub plan_arn: String,
    pub version_id: String,
    pub plan_name: String,
    pub creation_date: f64,
    pub last_execution_date: Option<f64>,
    pub rules: Vec<serde_json::Value>,
    pub advanced_settings: Option<serde_json::Value>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSelection {
    pub selection_id: String,
    pub plan_id: String,
    pub selection_name: String,
    pub iam_role_arn: String,
    pub resources: Vec<String>,
    pub list_of_tags: Vec<serde_json::Value>,
    pub conditions: Option<serde_json::Value>,
    pub creation_date: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupJob {
    pub job_id: String,
    pub backup_vault_name: String,
    pub backup_vault_arn: String,
    pub recovery_point_arn: String,
    pub resource_arn: String,
    pub creation_date: f64,
    pub completion_date: Option<f64>,
    pub state: String,
    pub status_message: Option<String>,
    pub percent_done: String,
    pub backup_size_in_bytes: u64,
    pub iam_role_arn: String,
    pub resource_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupStateSnapshot {
    pub vaults: Vec<BackupVault>,
    pub plans: Vec<BackupPlan>,
    pub selections: Vec<BackupSelection>,
    pub jobs: Vec<BackupJob>,
}

impl BackupState {
    pub fn to_snapshot(&self) -> BackupStateSnapshot {
        BackupStateSnapshot {
            vaults: self.vaults.iter().map(|e| e.value().clone()).collect(),
            plans: self.plans.iter().map(|e| e.value().clone()).collect(),
            selections: self.selections.iter().map(|e| e.value().clone()).collect(),
            jobs: self.jobs.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: BackupStateSnapshot) {
        self.vaults.clear();
        self.plans.clear();
        self.selections.clear();
        self.jobs.clear();
        for v in snap.vaults {
            self.vaults.insert(v.name.clone(), v);
        }
        for p in snap.plans {
            self.plans.insert(p.plan_id.clone(), p);
        }
        for s in snap.selections {
            let key = format!("{}:{}", s.plan_id, s.selection_id);
            self.selections.insert(key, s);
        }
        for j in snap.jobs {
            self.jobs.insert(j.job_id.clone(), j);
        }
    }
}
