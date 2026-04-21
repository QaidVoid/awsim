use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// IAM state — global per account (region is always "global" for IAM).
#[derive(Debug, Default)]
pub struct IamState {
    pub users: DashMap<String, User>,
    pub groups: DashMap<String, Group>,
    pub roles: DashMap<String, Role>,
    /// Keyed by ARN
    pub policies: DashMap<String, Policy>,
    pub instance_profiles: DashMap<String, InstanceProfile>,
}

/// Serializable snapshot of `IamState`.
#[derive(Debug, Serialize, Deserialize)]
pub struct IamStateSnapshot {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    pub roles: Vec<Role>,
    pub policies: Vec<Policy>,
    pub instance_profiles: Vec<InstanceProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_name: String,
    /// AIDA... format, 20 chars
    pub user_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    pub access_keys: Vec<AccessKey>,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
    /// group names this user belongs to
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub status: String,
    pub create_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_name: String,
    /// AGPA... format
    pub group_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    /// User names belonging to this group
    pub members: Vec<String>,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub role_name: String,
    /// AROA... format
    pub role_id: String,
    pub arn: String,
    pub path: String,
    pub assume_role_policy_document: String,
    pub description: Option<String>,
    pub create_date: String,
    /// ARNs of attached managed policies
    pub attached_policies: Vec<String>,
    /// name → document
    pub inline_policies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub policy_name: String,
    /// ANPA... format
    pub policy_id: String,
    pub arn: String,
    pub path: String,
    pub description: Option<String>,
    pub policy_document: String,
    pub create_date: String,
    pub update_date: String,
    /// How many entities are attached
    pub attachment_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProfile {
    pub instance_profile_name: String,
    /// AIPA... format
    pub instance_profile_id: String,
    pub arn: String,
    pub path: String,
    pub create_date: String,
    /// Role names associated
    pub roles: Vec<String>,
}
