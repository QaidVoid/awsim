use awsim_core::BodyStore;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone)]
pub enum FunctionCode {
    InMemory(Vec<u8>),
    OnDisk(PathBuf),
}

impl FunctionCode {
    pub fn read_all(&self) -> std::io::Result<Vec<u8>> {
        match self {
            FunctionCode::InMemory(b) => Ok(b.clone()),
            FunctionCode::OnDisk(p) => std::fs::read(p),
        }
    }

    pub fn len_hint(&self) -> Option<u64> {
        match self {
            FunctionCode::InMemory(b) => Some(b.len() as u64),
            FunctionCode::OnDisk(p) => std::fs::metadata(p).ok().map(|m| m.len()),
        }
    }
}

/// Lambda state — per account and region.
#[derive(Debug, Default)]
pub struct LambdaState {
    pub functions: DashMap<String, LambdaFunction>,
    pub event_source_mappings: DashMap<String, EventSourceMapping>,
    pub layers: DashMap<String, Vec<LayerVersion>>,
    /// function_name → FunctionUrlConfig
    pub url_configs: DashMap<String, FunctionUrlConfig>,
    /// function_name[:qualifier] → EventInvokeConfig
    pub event_invoke_configs: DashMap<String, EventInvokeConfig>,
    pub body_store: OnceLock<Arc<BodyStore>>,
}

impl LambdaState {
    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.get()
    }

    pub fn set_body_store(&self, store: Arc<BodyStore>) {
        let _ = self.body_store.set(store);
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventInvokeConfig {
    pub function_arn: String,
    pub maximum_retry_attempts: Option<i32>,
    pub maximum_event_age_in_seconds: Option<i32>,
    pub destination_on_success: Option<String>,
    pub destination_on_failure: Option<String>,
    pub last_modified: f64,
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
    pub code: Option<FunctionCode>,
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
    pub code: Option<FunctionCode>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct LambdaStateSnapshot {
    pub functions: Vec<FunctionSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionSnapshot {
    pub account_id: String,
    pub region: String,
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
    pub environment: HashMap<String, String>,
    pub version: String,
    pub versions: Vec<FunctionVersionSnapshot>,
    pub aliases: HashMap<String, AliasSnapshot>,
    pub last_modified: String,
    pub state: String,
    #[serde(default)]
    pub policy_statements: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionVersionSnapshot {
    pub version: String,
    pub description: String,
    pub code_sha256: String,
    pub code_size: u64,
    pub last_modified: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AliasSnapshot {
    pub name: String,
    pub arn: String,
    pub function_version: String,
    pub description: String,
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
