use dashmap::DashMap;
use std::collections::HashMap;

/// Lambda state — per account and region.
#[derive(Debug, Default)]
pub struct LambdaState {
    pub functions: DashMap<String, LambdaFunction>,
    pub event_source_mappings: DashMap<String, EventSourceMapping>,
    pub layers: DashMap<String, Vec<LayerVersion>>,
    /// function_name → FunctionUrlConfig
    pub url_configs: DashMap<String, FunctionUrlConfig>,
}

#[derive(Debug, Clone)]
pub struct LambdaFunction {
    pub name: String,
    pub arn: String,
    pub runtime: Option<String>,
    pub role: String,
    pub handler: Option<String>,
    pub description: String,
    pub timeout: u32,
    pub memory_size: u32,
    pub code_sha256: String,
    pub code_size: u64,
    pub code_data: Option<Vec<u8>>,
    pub environment: HashMap<String, String>,
    /// Always "$LATEST" for the live function.
    pub version: String,
    pub versions: Vec<FunctionVersion>,
    pub aliases: HashMap<String, Alias>,
    pub last_modified: String,
    /// "Active", "Pending", "Failed", etc.
    pub state: String,
    /// Invocation records for debugging / admin console.
    pub invocations: Vec<InvocationRecord>,
    /// Resource-based policy statements (for AddPermission / RemovePermission).
    pub policy_statements: HashMap<String, serde_json::Value>,
    /// Tags attached to this function.
    pub tags: HashMap<String, String>,
}

/// A function URL configuration.
#[derive(Debug, Clone)]
pub struct FunctionUrlConfig {
    /// Kept for potential admin console use.
    #[allow(dead_code)]
    pub function_name: String,
    pub function_arn: String,
    pub function_url: String,
    pub auth_type: String,
    pub cors: Option<serde_json::Value>,
    pub creation_time: String,
    pub last_modified_time: String,
}

#[derive(Debug, Clone)]
pub struct FunctionVersion {
    pub version: String,
    pub description: String,
    pub code_sha256: String,
    pub code_size: u64,
    pub last_modified: String,
}

#[derive(Debug, Clone)]
pub struct Alias {
    pub name: String,
    pub arn: String,
    pub function_version: String,
    pub description: String,
}

/// Stored for debugging and the admin console — fields read externally.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct InvocationRecord {
    pub invocation_id: String,
    pub invocation_type: String,
    pub payload: serde_json::Value,
    pub response: serde_json::Value,
    pub status_code: u16,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct EventSourceMapping {
    pub uuid: String,
    pub event_source_arn: String,
    pub function_arn: String,
    pub batch_size: u32,
    /// Stored for potential future use / admin console.
    #[allow(dead_code)]
    pub enabled: bool,
    pub state: String,
    pub last_modified: String,
}

#[derive(Debug, Clone)]
pub struct LayerVersion {
    /// Layer name kept for reference / admin console.
    #[allow(dead_code)]
    pub layer_name: String,
    pub layer_arn: String,
    pub version_arn: String,
    pub version: u64,
    pub description: String,
    pub compatible_runtimes: Vec<String>,
    pub code_sha256: String,
    pub code_size: u64,
    /// Raw zip bytes stored for future execution support.
    #[allow(dead_code)]
    pub code_data: Option<Vec<u8>>,
    pub created_date: String,
}
