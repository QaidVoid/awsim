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
}

/// Per-account/region SSM state.
#[derive(Debug, Default)]
pub struct SsmState {
    /// Parameter name → Parameter
    pub parameters: DashMap<String, Parameter>,
}
