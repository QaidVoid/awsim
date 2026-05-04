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
                                    routing_config: a.routing_config.clone(),
                                },
                            )
                        })
                        .collect(),
                    last_modified: f.last_modified.clone(),
                    state: f.state.clone(),
                    policy_statements: f.policy_statements.clone(),
                    tags: f.tags.clone(),
                    architectures: f.architectures.clone(),
                    ephemeral_storage_size: f.ephemeral_storage_size,
                    package_type: f.package_type.clone(),
                    layers: f.layers.clone(),
                    vpc_config: f.vpc_config.clone(),
                    dead_letter_config: f.dead_letter_config.clone(),
                    tracing_config: f.tracing_config.clone(),
                    kms_key_arn: f.kms_key_arn.clone(),
                    file_system_configs: f.file_system_configs.clone(),
                    logging_config: f.logging_config.clone(),
                    snap_start: f.snap_start.clone(),
                    image_config: f.image_config.clone(),
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
                            routing_config: a.routing_config,
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
                reserved_concurrent_executions: None,
                provisioned_concurrency: HashMap::new(),
                architectures: fs.architectures,
                ephemeral_storage_size: fs.ephemeral_storage_size,
                package_type: fs.package_type,
                layers: fs.layers,
                vpc_config: fs.vpc_config,
                dead_letter_config: fs.dead_letter_config,
                tracing_config: fs.tracing_config,
                kms_key_arn: fs.kms_key_arn,
                file_system_configs: fs.file_system_configs,
                logging_config: fs.logging_config,
                snap_start: fs.snap_start,
                image_config: fs.image_config,
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
    /// Reserved concurrent executions ceiling per PutFunctionConcurrency.
    /// `None` means unreserved — the function shares the account pool.
    pub reserved_concurrent_executions: Option<u32>,
    /// Provisioned concurrency configurations keyed by qualifier (alias name
    /// or function version). Each entry tracks the requested capacity along
    /// with a simulated state machine that flips IN_PROGRESS -> READY.
    pub provisioned_concurrency: HashMap<String, ProvisionedConcurrencyConfig>,
    /// CPU architecture set: `["x86_64"]` or `["arm64"]`. Defaults to
    /// `["x86_64"]` per AWS.
    pub architectures: Vec<String>,
    /// `/tmp` size in MiB. Defaults to 512; AWS allows 512..=10240.
    pub ephemeral_storage_size: u32,
    /// "Zip" or "Image". Defaults to "Zip".
    pub package_type: String,
    /// Optional layer-version ARNs attached to the function.
    pub layers: Vec<String>,
    /// VpcConfig as supplied by the caller plus the synthesized VpcId field.
    pub vpc_config: Option<serde_json::Value>,
    /// DeadLetterConfig (`{ TargetArn }`).
    pub dead_letter_config: Option<serde_json::Value>,
    /// TracingConfig (`{ Mode }`). Defaults to `{ Mode: "PassThrough" }`.
    pub tracing_config: Option<serde_json::Value>,
    /// KMS key ARN used to encrypt environment variables at rest.
    pub kms_key_arn: Option<String>,
    /// EFS mounts: array of `{ Arn, LocalMountPath }`.
    pub file_system_configs: Option<serde_json::Value>,
    /// LoggingConfig (`{ LogFormat, ApplicationLogLevel, SystemLogLevel, LogGroup }`).
    pub logging_config: Option<serde_json::Value>,
    /// SnapStart configuration. Stored as supplied; the serializer adds the
    /// computed `OptimizationStatus` field.
    pub snap_start: Option<serde_json::Value>,
    /// ImageConfig for container-image functions.
    pub image_config: Option<serde_json::Value>,
}

/// Provisioned concurrency configuration for a single (function, qualifier)
/// pair. Real Lambda transitions IN_PROGRESS -> READY asynchronously; we
/// flip immediately because the emulator never has provisioning latency.
#[derive(Debug, Clone)]
pub struct ProvisionedConcurrencyConfig {
    pub qualifier: String,
    pub requested_provisioned_concurrent_executions: u32,
    pub allocated_provisioned_concurrent_executions: u32,
    pub available_provisioned_concurrent_executions: u32,
    pub status: String, // IN_PROGRESS | READY | FAILED
    pub status_reason: Option<String>,
    pub last_modified: String,
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
    /// Traffic-shifting weights: `version → fraction in [0, 1]`. When set,
    /// invocations through the alias split between `function_version` and
    /// the listed versions per their weights. Must total ≤ 1; the
    /// implicit remainder is routed to `function_version`.
    pub routing_config: HashMap<String, f64>,
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
    /// TRIM_HORIZON | LATEST | AT_TIMESTAMP — only meaningful for Kinesis/DDB streams.
    pub starting_position: Option<String>,
    pub starting_position_timestamp: Option<f64>,
    pub maximum_batching_window_in_seconds: u32,
    pub maximum_record_age_in_seconds: Option<i32>,
    pub bisect_batch_on_function_error: bool,
    pub maximum_retry_attempts: Option<i32>,
    pub parallelization_factor: Option<u32>,
    pub tumbling_window_in_seconds: Option<u32>,
    /// Raw FilterCriteria JSON: { "Filters": [{ "Pattern": "..." }, ...] }.
    pub filter_criteria: Option<serde_json::Value>,
    /// DestinationConfig.OnFailure.Destination ARN — receives failed batches.
    pub destination_on_failure: Option<String>,
    pub function_response_types: Vec<String>,
    /// Last poll result, surfaced via Get/List for diagnostics.
    /// "OK", "PROBLEM: <message>", or "No records processed".
    pub last_processing_result: String,
    /// Per-shard iterator state for Kinesis/DDB-stream pollers so we don't
    /// re-deliver records on every tick. Keyed by shard id.
    pub shard_iterators: HashMap<String, String>,
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
    #[serde(default = "default_architectures")]
    pub architectures: Vec<String>,
    #[serde(default = "default_ephemeral_storage_size")]
    pub ephemeral_storage_size: u32,
    #[serde(default = "default_package_type")]
    pub package_type: String,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub vpc_config: Option<serde_json::Value>,
    #[serde(default)]
    pub dead_letter_config: Option<serde_json::Value>,
    #[serde(default)]
    pub tracing_config: Option<serde_json::Value>,
    #[serde(default)]
    pub kms_key_arn: Option<String>,
    #[serde(default)]
    pub file_system_configs: Option<serde_json::Value>,
    #[serde(default)]
    pub logging_config: Option<serde_json::Value>,
    #[serde(default)]
    pub snap_start: Option<serde_json::Value>,
    #[serde(default)]
    pub image_config: Option<serde_json::Value>,
}

fn default_architectures() -> Vec<String> {
    vec!["x86_64".to_string()]
}

fn default_ephemeral_storage_size() -> u32 {
    512
}

fn default_package_type() -> String {
    "Zip".to_string()
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
    #[serde(default)]
    pub routing_config: HashMap<String, f64>,
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
