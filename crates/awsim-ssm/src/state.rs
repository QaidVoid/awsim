use std::collections::HashMap;

use dashmap::DashMap;

/// A single version entry for a parameter.
#[derive(Debug, Clone)]
pub struct ParameterVersion {
    pub value: String,
    pub version: u64,
    /// Unix epoch seconds (stored as u64, serialised as a JSON number).
    pub date: u64,
    pub description: String,
    /// Labels attached to this version.
    pub labels: Vec<String>,
}

/// A stored SSM parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub arn: String,
    pub param_type: String, // String, StringList, SecureString
    pub value: String,
    pub description: String,
    pub version: u64,
    /// Unix epoch seconds (stored as u64, serialised as a JSON number).
    pub last_modified_date: u64,
    pub tags: HashMap<String, String>,
    pub history: Vec<ParameterVersion>,
    pub tier: String,
    /// Labels on the current version.
    pub labels: Vec<String>,
}

/// A stored SSM Run Command record (stub).
#[derive(Debug, Clone)]
pub struct Command {
    pub command_id: String,
    pub document_name: String,
    pub targets: Vec<serde_json::Value>,
    pub status: String,
    pub created_time: u64,
}

/// Per-account/region SSM state.
#[derive(Debug, Default)]
pub struct SsmState {
    /// Parameter name → Parameter
    pub parameters: DashMap<String, Parameter>,
    /// CommandId → Command
    pub commands: DashMap<String, Command>,
}
