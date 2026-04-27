use awsim_core::{Body, BodyStore, Snapshottable};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

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

impl Snapshottable for LambdaState {
    type Snapshot = LambdaRegionSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        let functions = self
            .functions
            .iter()
            .map(|entry| {
                let f = entry.value();
                FunctionSnapshot {
                    account_id: account_id.to_string(),
                    region: region.to_string(),
                    name: f.name.clone(),
                    arn: f.arn.clone(),
                    runtime: f.runtime.clone(),
                    role: f.role.clone(),
                    handler: f.handler.clone(),
                    description: f.description.clone(),
                    timeout: f.timeout,
                    memory_size: f.memory_size,
                    code_sha256: f.code_sha256.clone(),
                    code_size: f.code_size,
                    environment: f.environment.clone(),
                    version: f.version.clone(),
                    versions: f
                        .versions
                        .iter()
                        .map(|v| FunctionVersionSnapshot {
                            version: v.version.clone(),
                            description: v.description.clone(),
                            code_sha256: v.code_sha256.clone(),
                            code_size: v.code_size,
                            last_modified: v.last_modified.clone(),
                        })
                        .collect(),
                    aliases: f
                        .aliases
                        .iter()
                        .map(|(k, a)| {
                            (
                                k.clone(),
                                AliasSnapshot {
                                    name: a.name.clone(),
                                    arn: a.arn.clone(),
                                    function_version: a.function_version.clone(),
                                    description: a.description.clone(),
                                },
                            )
                        })
                        .collect(),
                    last_modified: f.last_modified.clone(),
                    state: f.state.clone(),
                    policy_statements: f.policy_statements.clone(),
                    tags: f.tags.clone(),
                }
            })
            .collect();

        LambdaRegionSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            functions,
        }
    }

    fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self) {
        let state = LambdaState::default();
        for fs in snapshot.functions {
            let versions: Vec<FunctionVersion> = fs
                .versions
                .into_iter()
                .map(|v| FunctionVersion {
                    version: v.version,
                    description: v.description,
                    code_sha256: v.code_sha256,
                    code_size: v.code_size,
                    code: None,
                    last_modified: v.last_modified,
                })
                .collect();

            let aliases: HashMap<String, Alias> = fs
                .aliases
                .into_iter()
                .map(|(k, a)| {
                    (
                        k,
                        Alias {
                            name: a.name,
                            arn: a.arn,
                            function_version: a.function_version,
                            description: a.description,
                        },
                    )
                })
                .collect();

            let func = LambdaFunction {
                name: fs.name.clone(),
                arn: fs.arn,
                runtime: fs.runtime,
                role: fs.role,
                handler: fs.handler,
                description: fs.description,
                timeout: fs.timeout,
                memory_size: fs.memory_size,
                code_sha256: fs.code_sha256,
                code_size: fs.code_size,
                code: None,
                environment: fs.environment,
                version: fs.version,
                versions,
                aliases,
                last_modified: fs.last_modified,
                state: fs.state,
                invocations: Vec::new(),
                policy_statements: fs.policy_statements,
                tags: fs.tags,
            };
            state.functions.insert(fs.name, func);
        }
        (snapshot.account_id, snapshot.region, state)
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
    pub code: Option<Body>,
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
    pub code: Option<Body>,
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
pub struct LambdaRegionSnapshot {
    pub account_id: String,
    pub region: String,
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
