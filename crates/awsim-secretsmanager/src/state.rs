use dashmap::DashMap;
use std::collections::HashMap;

/// A single version of a secret value.
#[derive(Debug, Clone)]
pub struct SecretVersion {
    pub version_id: String,
    pub secret_string: Option<String>,
    /// base64-encoded binary value
    pub secret_binary: Option<String>,
    /// e.g. ["AWSCURRENT"], ["AWSPREVIOUS"]
    pub stages: Vec<String>,
    pub created_date: String,
}

/// A secret and all its versions.
#[derive(Debug, Clone)]
pub struct Secret {
    pub arn: String,
    pub name: String,
    pub description: String,
    /// version_id → SecretVersion
    pub versions: HashMap<String, SecretVersion>,
    pub current_version_id: String,
    pub tags: HashMap<String, String>,
    pub created_date: String,
    pub last_changed_date: String,
    pub deleted_date: Option<String>,
}

/// Per-account/region Secrets Manager state.
#[derive(Debug, Default)]
pub struct SecretsState {
    /// name → Secret (primary index)
    pub secrets: DashMap<String, Secret>,
}
