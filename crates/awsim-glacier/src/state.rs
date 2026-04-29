use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct GlacierState {
    pub vaults: DashMap<String, Vault>,
    /// (vault, archive_id) keyed.
    pub archives: DashMap<String, Archive>,
    /// (vault, job_id) keyed.
    pub jobs: DashMap<String, Job>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub vault_name: String,
    pub vault_arn: String,
    pub creation_date: String,
    pub last_inventory_date: Option<String>,
    pub number_of_archives: u64,
    pub size_in_bytes: u64,
    pub notification_topic: Option<String>,
    pub notification_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub vault_name: String,
    pub archive_id: String,
    pub creation_date: String,
    pub size: u64,
    pub sha256_tree_hash: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub vault_name: String,
    pub job_id: String,
    pub action: String,
    pub archive_id: Option<String>,
    pub status_code: String,
    pub creation_date: String,
    pub completion_date: Option<String>,
    pub status_message: Option<String>,
    pub job_description: Option<String>,
    pub sns_topic: Option<String>,
    pub tier: Option<String>,
}

pub fn archive_key(vault: &str, archive_id: &str) -> String {
    format!("{vault}|{archive_id}")
}
pub fn job_key(vault: &str, job_id: &str) -> String {
    format!("{vault}|{job_id}")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlacierSnapshot {
    pub vaults: Vec<Vault>,
    pub archives: Vec<Archive>,
    pub jobs: Vec<Job>,
}

impl GlacierState {
    pub fn to_snapshot(&self) -> GlacierSnapshot {
        GlacierSnapshot {
            vaults: self.vaults.iter().map(|e| e.value().clone()).collect(),
            archives: self.archives.iter().map(|e| e.value().clone()).collect(),
            jobs: self.jobs.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: GlacierSnapshot) {
        self.vaults.clear();
        self.archives.clear();
        self.jobs.clear();
        for v in snap.vaults {
            self.vaults.insert(v.vault_name.clone(), v);
        }
        for a in snap.archives {
            self.archives
                .insert(archive_key(&a.vault_name, &a.archive_id), a);
        }
        for j in snap.jobs {
            self.jobs.insert(job_key(&j.vault_name, &j.job_id), j);
        }
    }
}
