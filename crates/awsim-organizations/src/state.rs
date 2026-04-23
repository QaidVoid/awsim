use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct OrganizationsState {
    pub organization: std::sync::RwLock<Option<Organization>>,
    pub accounts: DashMap<String, Account>,
    pub ous: DashMap<String, OrganizationalUnit>,
    pub policies: DashMap<String, Policy>,
    pub policy_attachments: DashMap<String, Vec<String>>,
    pub roots: DashMap<String, Root>,
    pub parents: DashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub arn: String,
    pub feature_set: String,
    pub master_account_id: String,
    pub master_account_arn: String,
    pub master_account_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub arn: String,
    pub email: String,
    pub name: String,
    pub status: String,
    pub joined_method: String,
    pub joined_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnit {
    pub id: String,
    pub arn: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub arn: String,
    pub name: String,
    pub description: String,
    pub policy_type: String,
    pub content: String,
    pub aws_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    pub id: String,
    pub arn: String,
    pub name: String,
    pub policy_types: Vec<HashMap<String, String>>,
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
