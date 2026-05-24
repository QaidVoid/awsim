use anyhow::{Context, Result};
use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::limit::ConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;
use tracing::{debug, error, info, warn};

// jemalloc returns memory to the OS more aggressively than glibc
// malloc, so idle RSS stays flat after burst workloads (DDB query
// loops, bulk imports). MSVC builds keep the system allocator since
// jemalloc isn't well supported there.
#[cfg(all(not(target_env = "msvc"), not(target_env = "musl")))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use awsim_core::{
    AppState, BlobInventory, BodyStore, BodyStoreHandle, PersistenceManager, RequestContext,
};

mod admin;
mod bill_cli;
mod chaos_cli;
mod integrations;
mod named_snapshots;
mod operator_auth;
mod proxy;
mod runtime_config;
mod seed;
mod seed_cli;
mod snapshot_cli;
mod tls;
mod ui;

#[derive(Parser)]
#[command(
    name = "awsim",
    about = "AWSim — fully offline, free AWS development environment"
)]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "4566", env = "AWSIM_PORT")]
    port: u16,

    /// HTTPS port. When set, AWSim serves the same router on this
    /// port with a self-signed cert (or `--tls-cert` / `--tls-key` if
    /// provided). The plain `--port` listener stays up so existing
    /// `http://` clients keep working.
    #[arg(long, env = "AWSIM_HTTPS_PORT")]
    https_port: Option<u16>,

    /// PEM-encoded TLS certificate (BYO). When set with `--tls-key`,
    /// AWSim skips self-signed-cert generation and serves this cert
    /// instead. Useful if you already have a locally-trusted CA via
    /// `mkcert` or similar.
    #[arg(long, env = "AWSIM_TLS_CERT", requires = "tls_key")]
    tls_cert: Option<std::path::PathBuf>,

    /// PEM-encoded TLS private key (BYO). Pair with `--tls-cert`.
    #[arg(long, env = "AWSIM_TLS_KEY", requires = "tls_cert")]
    tls_key: Option<std::path::PathBuf>,

    /// Where to cache the auto-generated TLS cert + key. Defaults to
    /// `<data-dir>/tls` if `--data-dir` is set, else
    /// `$XDG_CACHE_HOME/awsim/tls` (or `$HOME/.cache/awsim/tls`).
    /// Ignored when `--tls-cert` / `--tls-key` are set.
    #[arg(long, env = "AWSIM_TLS_CACHE_DIR")]
    tls_cache_dir: Option<std::path::PathBuf>,

    /// Default AWS region
    #[arg(short, long, default_value = "us-east-1", env = "AWSIM_REGION")]
    region: String,

    /// Default AWS account ID
    #[arg(long, default_value = "000000000000", env = "AWSIM_ACCOUNT_ID")]
    account_id: String,

    /// Default AWS partition: `aws`, `aws-cn`, `aws-us-gov`, `aws-iso`,
    /// or `aws-iso-b`. Reflected in every emitted ARN.
    #[arg(long, default_value = "aws", env = "AWSIM_PARTITION")]
    partition: String,

    /// Data directory for persistence (omit for in-memory only)
    #[arg(long, env = "AWSIM_DATA_DIR")]
    data_dir: Option<String>,

    /// Log level
    #[arg(short = 'v', long, default_value = "info", env = "AWSIM_LOG_LEVEL")]
    log_level: String,

    /// Disable startup garbage collection of orphaned BodyStore blobs
    #[arg(long, env = "AWSIM_NO_GC", default_value_t = false)]
    no_gc: bool,

    /// Per-service on-disk blob cap in bytes (FIFO eviction by mtime when exceeded).
    /// Applied independently to S3, Lambda, SQS, ECR. Unset = unbounded.
    #[arg(long, env = "AWSIM_MAX_BLOB_BYTES")]
    max_blob_bytes: Option<u64>,

    /// Re-run BodyStore orphan GC every N seconds (in addition to startup).
    /// Unset = startup-only GC.
    #[arg(long, env = "AWSIM_GC_INTERVAL_SECS")]
    gc_interval_secs: Option<u64>,

    /// Force a DynamoDB SQLite WAL TRUNCATE checkpoint every N seconds,
    /// bounding `-wal` growth (and its memory) under sustained write
    /// bursts like bulk imports. Unset = 60s; 0 disables.
    #[arg(long, env = "AWSIM_DDB_WAL_CHECKPOINT_SECS")]
    ddb_wal_checkpoint_secs: Option<u64>,

    /// Maximum concurrent in-flight HTTP requests. Requests above this cap
    /// are immediately rejected with 503 Service Unavailable instead of
    /// queuing — so a misbehaving client (e.g. one leaking connections
    /// during a bulk import) can't accumulate work that eventually
    /// exhausts file descriptors or memory.
    #[arg(long, env = "AWSIM_MAX_CONCURRENT_REQUESTS", default_value_t = 5_000)]
    max_concurrent_requests: usize,

    /// Cap on tokio's blocking-pool threads (the pool that runs sync
    /// SQLite calls via `spawn_blocking`). Each thread reserves ~2 MiB
    /// of stack, so this directly bounds RSS contribution from
    /// blocking work. Set lower (e.g. 8) to clamp memory during bulk
    /// imports; raise for higher write throughput.
    #[arg(long, env = "AWSIM_MAX_BLOCKING_THREADS", default_value_t = 32)]
    max_blocking_threads: usize,

    /// Per-request body size cap in bytes for non-S3-upload routes.
    /// axum aborts the body stream once this many bytes have arrived
    /// and returns 413, so memory per request is bounded by what the
    /// client actually transmits up to the cap. Default 100 MiB.
    #[arg(long, env = "AWSIM_MAX_BODY_BYTES", default_value_t = 100 * 1024 * 1024)]
    max_body_bytes: usize,

    /// Body cap for S3 PutObject / UploadPart routes specifically.
    /// S3 single-PUT objects can legitimately reach 5 GiB and multipart
    /// parts up to 5 GiB each; raising the global `--max-body-bytes`
    /// to that level would let a misbehaving non-S3 client buffer a
    /// gigabyte under any other endpoint, so we apply this larger cap
    /// only on bucket/key-shaped paths via a route layer. Default 5 GiB.
    #[arg(
        long,
        env = "AWSIM_MAX_S3_UPLOAD_BYTES",
        default_value_t = 5 * 1024 * 1024 * 1024
    )]
    max_s3_upload_bytes: usize,

    /// Hours to retain captured SES outbound emails before the
    /// hourly sweep deletes them. Default 720 (30 days). Set to 0
    /// to disable the sweep entirely.
    #[arg(long, env = "AWSIM_SES_RETENTION_HOURS", default_value_t = 720)]
    ses_retention_hours: u64,

    /// OpenAI-compatible base URL for the Bedrock proxy backend
    /// (e.g. `http://localhost:11434/v1` for Ollama,
    /// `http://localhost:1234/v1` for LM Studio). When unset,
    /// Bedrock InvokeModel returns deterministic canned responses
    /// so SDK wiring can still be tested without a local LLM.
    #[arg(long, env = "AWSIM_BEDROCK_BACKEND")]
    bedrock_backend: Option<String>,

    /// Optional API key for the Bedrock proxy backend, sent as
    /// `Authorization: Bearer <key>`. Most local servers (Ollama
    /// default, LM Studio default) don't need this.
    #[arg(long, env = "AWSIM_BEDROCK_API_KEY")]
    bedrock_api_key: Option<String>,

    /// Path to a TOML file overriding the built-in Bedrock model
    /// map. Keys are AWS-style ids (`anthropic.claude-3-5-sonnet-…`)
    /// and values are backend-side model tags (`llama3.1:8b`).
    /// User overrides merge on top of the defaults; see the
    /// Bedrock guide for the file shape.
    #[arg(long, env = "AWSIM_BEDROCK_MODEL_MAP")]
    bedrock_model_map: Option<std::path::PathBuf>,

    /// Path to a TOML config file declaring multiple Bedrock proxy
    /// backends and per-id routing in one place. Takes precedence over
    /// `--bedrock-backend` / `--bedrock-api-key` / `--bedrock-model-map`.
    /// See the Bedrock guide for the schema.
    #[arg(long, env = "AWSIM_BEDROCK_CONFIG")]
    bedrock_config: Option<std::path::PathBuf>,

    /// Force IAM enforcement on at startup, regardless of the persisted
    /// runtime config. Useful for containers where the operator wants
    /// IAM policies enforced from boot without first opening the UI.
    /// The setting is still hot-reloadable from the UI afterwards.
    #[arg(long, env = "AWSIM_ENFORCE_IAM")]
    enforce_iam: Option<bool>,

    /// Access key that bypasses IAM enforcement, modeling AWS root
    /// credentials. The management UI and bootstrap flows sign with
    /// this key so they keep working once enforcement is on, even
    /// before any IAM users exist. Set to an empty string to disable.
    #[arg(long, env = "AWSIM_ADMIN_ACCESS_KEY", default_value = "awsim-admin")]
    admin_access_key: String,

    /// One-shot subcommand. Without one, `awsim` runs the server
    /// (the default for backwards compatibility).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print the current bill from a running awsim instance.
    Bill {
        /// awsim endpoint to query.
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        /// Emit raw JSON instead of the human-readable summary.
        #[arg(long)]
        json: bool,
    },
    /// Manage chaos-injection rules on a running awsim instance.
    Chaos {
        #[command(subcommand)]
        command: ChaosCommand,
    },
    /// Save / load named state snapshots — point-in-time bundles of
    /// every service's serialised state, plus billing + chaos.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Reclaim disk space in the DynamoDB SQLite store. Run after
    /// heavy DELETE / UPDATE churn — the file shrinks back to live
    /// data size.
    Vacuum {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
    },
    /// Bulk-seed services from a TOML scenario file via /_awsim/seed.
    Seed {
        /// Path to the TOML config (see `awsim seed --help` for shape).
        #[arg(long)]
        file: std::path::PathBuf,
        /// Override the endpoint from the TOML file.
        #[arg(long, env = "AWSIM_ENDPOINT")]
        endpoint: Option<String>,
    },
}

#[derive(Subcommand)]
enum SnapshotCommand {
    /// List saved snapshots.
    List {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        #[arg(long)]
        json: bool,
    },
    /// Save the current state under NAME (overwrites if it exists).
    Save {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        /// Snapshot name — ASCII alnum + `-` + `_`, max 64 chars.
        name: String,
    },
    /// Restore state from NAME. Existing live state is overwritten
    /// for any account/region/service represented in the snapshot.
    Load {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        name: String,
    },
    /// Delete a saved snapshot.
    Delete {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        name: String,
    },
}

#[derive(Subcommand)]
enum ChaosCommand {
    /// List active rules.
    List {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        #[arg(long)]
        json: bool,
    },
    /// Add a new rule. Specify `--error`, `--latency`, or both.
    Add {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        /// Service signing name (e.g. `s3`) or `*` for any.
        #[arg(long, default_value = "*")]
        service: String,
        /// Operation name (e.g. `PutObject`) or `*` for any.
        #[arg(long, default_value = "*")]
        operation: String,
        /// Probability in `[0.0, 1.0]`. Defaults to 1.0 (always fires).
        #[arg(long, default_value_t = 1.0)]
        probability: f64,
        /// Error spec: `STATUS,CODE[,MESSAGE]`. Example: `503,SlowDown,please retry`.
        #[arg(long)]
        error: Option<String>,
        /// Latency range in ms. `100` for fixed, `100-500` for a range.
        #[arg(long)]
        latency: Option<String>,
        /// Optional human label shown in the dashboard.
        #[arg(long)]
        label: Option<String>,
        /// Auto-stop firing after N seconds (rule stays in the table
        /// but gates itself off). Useful for "fire for the next 5
        /// min then stop".
        #[arg(long)]
        ttl_secs: Option<u64>,
        /// Wait N seconds before the rule starts firing.
        #[arg(long)]
        start_in_secs: Option<u64>,
        /// Periodic on/off cycle: `ACTIVE/PERIOD` in seconds. e.g.
        /// `30/60` = on for 30s out of every 60s.
        #[arg(long)]
        flap: Option<String>,
    },
    /// Remove a rule by id.
    Remove {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        id: String,
    },
    /// Clear all rules + reset injection counters.
    Clear {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
    },
    /// Show injection stats (total + recent ring buffer).
    Stats {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
    },
    /// Built-in chaos presets — list or apply common failure scenarios.
    Preset {
        #[command(subcommand)]
        command: ChaosPresetCommand,
    },
}

#[derive(Subcommand)]
enum ChaosPresetCommand {
    /// List the available presets.
    List {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        #[arg(long)]
        json: bool,
    },
    /// Apply a preset by name (e.g. `flaky-s3`).
    Apply {
        #[arg(long, default_value = "http://localhost:4566", env = "AWSIM_ENDPOINT")]
        endpoint: String,
        /// Preset name (see `awsim chaos preset list`).
        name: String,
    },
}

fn main() -> Result<()> {
    // Peek at the CLI just to size the runtime. Full parse happens
    // inside `async_main`. The tokio default of 512 blocking threads
    // × 2 MiB stack ≈ 1 GiB ceiling for `spawn_blocking` is easy to
    // hit during a bulk DDB import that fans out across many sync
    // SQLite calls — keep it tight by default and let users override.
    let max_blocking = Cli::try_parse()
        .map(|c| c.max_blocking_threads)
        .unwrap_or(32);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(max_blocking)
        .build()?;
    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    let cli = Cli::parse();

    // One-shot subcommands run before the server-startup machinery
    // boots so they don't drag in tracing, the rlimit bump, or any
    // of the body-store wiring.
    if let Some(cmd) = cli.command {
        match cmd {
            Command::Bill { endpoint, json } => {
                return bill_cli::run(&endpoint, json).await;
            }
            Command::Chaos { command } => {
                return chaos_cli::run(command).await;
            }
            Command::Snapshot { command } => {
                return snapshot_cli::run(command).await;
            }
            Command::Vacuum { endpoint } => {
                return run_vacuum(&endpoint).await;
            }
            Command::Seed { file, endpoint } => {
                return seed_cli::run(&file, endpoint.as_deref()).await;
            }
        }
    }

    // Reloadable log filter: wrap the env filter in a `reload::Layer`
    // so the runtime-config hook can swap directives at runtime
    // without restarting the process.
    let initial_filter =
        tracing_subscriber::EnvFilter::try_new(&cli.log_level).unwrap_or_else(|e| {
            eprintln!(
                "Invalid log filter {:?}: {e} — falling back to 'info'",
                cli.log_level
            );
            tracing_subscriber::EnvFilter::new("info")
        });
    let (filter_layer, log_reload) = tracing_subscriber::reload::Layer::new(initial_filter);
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    raise_nofile_limit();

    let mut state = AppState::with_partition(
        cli.region.clone(),
        cli.account_id.clone(),
        cli.partition.clone(),
    );

    // Runtime config store — disk-backed when --data-dir is set, in
    // memory only otherwise. CLI flags seed initial values; persisted
    // file overlays them on subsequent runs.
    let runtime_config_store = build_runtime_config_store(&cli)?;
    info!(
        persistent = runtime_config_store.is_persistent(),
        path = ?runtime_config_store.config_path(),
        "Runtime config store initialised"
    );

    // Process-lifetime health registry. Survives across every
    // BedrockBackends hot-swap so statuses don't reset on config
    // reload. Read by the alias resolver (skip Down targets) and
    // by the gateway Health tab.
    let bedrock_health = awsim_bedrock::HealthRegistry::new();
    let bedrock_metrics = awsim_bedrock::MetricsRegistry::new();
    let bedrock_recent = awsim_bedrock::RecentInvocations::new();

    // Hot-swappable Bedrock backends handle. Built once from the
    // initial runtime config and swapped in-place whenever the
    // runtime config changes — request-path readers see the new
    // value without restarting the service.
    let initial_bedrock = build_bedrock_backend_from_config(
        &runtime_config_store.current(),
        &bedrock_health,
        &bedrock_metrics,
        &bedrock_recent,
    )?;
    let bedrock_swap = awsim_bedrock::backends_swap(initial_bedrock);

    // Reload hook: rebuild backends from the new config and swap.
    // Validation already ran in `apply()` so build failures here are
    // unexpected; we log and keep the previous registry to avoid
    // wedging the runtime on a transient env-var read.
    {
        let swap = Arc::clone(&bedrock_swap);
        let health_for_reload = bedrock_health.clone();
        let metrics_for_reload = bedrock_metrics.clone();
        let recent_for_reload = bedrock_recent.clone();
        runtime_config_store.on_change(Box::new(
            move |cfg| match build_bedrock_backend_from_config(
                cfg,
                &health_for_reload,
                &metrics_for_reload,
                &recent_for_reload,
            ) {
                Ok(next) => {
                    swap.store(Arc::new(next));
                    info!("Bedrock backends hot-reloaded");
                }
                Err(e) => {
                    warn!(error = %e, "Bedrock hot-reload failed; keeping previous registry")
                }
            },
        ));
    }

    // Background health poller. One tokio task pings each
    // configured backend's /models every 30s and records the
    // outcome in the shared registry. Survives every config swap
    // by watching the swap handle itself.
    {
        let swap = Arc::clone(&bedrock_swap);
        let registry = bedrock_health.clone();
        tokio::spawn(async move {
            awsim_bedrock::run_poller(
                swap,
                registry,
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(5),
            )
            .await;
        });
    }

    // Register all services; get back the ApiGateway Arc for proxy routing and
    // an Arc<CognitoState> for the default account+region so the OAuth router
    // can share user-pool state with the CognitoService.
    let (
        apigw_service,
        apigw_v1_service,
        cognito_state,
        iam_service,
        iam_store,
        s3_store,
        kms_store,
        sqs_store,
        secrets_store,
        lambda_store,
        organizations_store,
        ecr_service,
        s3_service,
        lambda_service,
        sqs_service,
        logs_service,
        pipes_store,
        ec2_service,
        rds_service,
        mq_service,
        memorydb_service,
        dynamodb_service,
        cw_metrics_service,
        kinesis_service,
        ses_service,
        sts_sessions,
    ) = register_services(
        &mut state,
        &cli.account_id,
        &cli.region,
        cli.data_dir.as_deref(),
        cli.port,
        cli.max_blob_bytes,
        cli.ddb_wal_checkpoint_secs,
        Arc::clone(&bedrock_swap),
    );

    let mut body_stores: Vec<BodyStoreHandle> = Vec::new();
    if let Some(bs) = s3_service.body_store() {
        body_stores.push(BodyStoreHandle {
            service_name: "s3".to_string(),
            groups: awsim_s3::S3Service::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
    if let Some(bs) = lambda_service.body_store() {
        body_stores.push(BodyStoreHandle {
            service_name: "lambda".to_string(),
            groups: awsim_lambda::LambdaService::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
    if let Some(bs) = sqs_service.body_store() {
        body_stores.push(BodyStoreHandle {
            service_name: "sqs".to_string(),
            groups: awsim_sqs::SqsService::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
    if let Some(bs) = ecr_service.body_store() {
        body_stores.push(BodyStoreHandle {
            service_name: "ecr".to_string(),
            groups: awsim_ecr::EcrService::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
    // CloudWatch Logs no longer uses a body store — events are in
    // its own SQLite file.
    state.body_stores = Arc::new(body_stores);
    if let Some(ref dir) = cli.data_dir {
        state.data_dir = Some(Arc::new(std::path::PathBuf::from(dir)));
    }

    // Billing meter: subscribes to the request-event stream and tallies
    // per-service / per-operation counters. Restored from disk + auto-saved
    // alongside the regular service snapshots when --data-dir is set.
    let billing_meter = Arc::new(awsim_billing::BillingMeter::new());
    awsim_billing::spawn_meter((*billing_meter).clone(), &state.events);

    if let Some(authz) = Arc::get_mut(&mut state.authz) {
        authz.admin_access_key =
            (!cli.admin_access_key.is_empty()).then(|| cli.admin_access_key.clone());
        let iam_lookup: Arc<dyn awsim_core::PrincipalLookup> =
            Arc::new(awsim_iam::authz::IamPrincipalLookup::new(iam_store));
        authz.principal_lookup = Arc::new(awsim_sts::StsAwarePrincipalLookup::new(
            Arc::clone(&sts_sessions),
            iam_lookup,
        ));
        authz.resource_policy_lookups.insert(
            "s3".to_string(),
            Arc::new(awsim_s3::S3ResourcePolicyLookup::new(s3_store)),
        );
        authz.resource_policy_lookups.insert(
            "kms".to_string(),
            Arc::new(awsim_kms::KmsResourcePolicyLookup::new(kms_store.clone())),
        );
        authz.grant_lookups.insert(
            "kms".to_string(),
            Arc::new(awsim_kms::KmsGrantLookup::new(kms_store)),
        );
        authz.resource_policy_lookups.insert(
            "sqs".to_string(),
            Arc::new(awsim_sqs::SqsResourcePolicyLookup::new(sqs_store.clone())),
        );
        authz.resource_policy_lookups.insert(
            "secretsmanager".to_string(),
            Arc::new(
                awsim_secretsmanager::SecretsManagerResourcePolicyLookup::new(
                    secrets_store.clone(),
                ),
            ),
        );
        authz.resource_policy_lookups.insert(
            "lambda".to_string(),
            Arc::new(awsim_lambda::LambdaResourcePolicyLookup::new(
                lambda_store.clone(),
            )),
        );
        authz.scp_lookup = Some(Arc::new(awsim_organizations::OrganizationsScpLookup::new(
            organizations_store,
            &cli.account_id,
        )));
    }

    // Now that the authz engine is fully built (lookups + SCP + grants),
    // hand a clone to the IAM service so its policy simulator can
    // evaluate against the same lookups the live request path uses —
    // identity policies *plus* resource policies, SCPs, KMS grants.
    iam_service.set_authz(Arc::clone(&state.authz));

    // Apply the runtime-config IAM enforce flag, then register a hook
    // so flipping it from the UI takes effect on the next request.
    // `--enforce-iam` / `AWSIM_ENFORCE_IAM` overrides whatever was
    // persisted, and also writes the override back into the runtime
    // config so the UI shows the same state.
    {
        let authz = Arc::clone(&state.authz);
        if let Some(forced) = cli.enforce_iam {
            let mut cfg = runtime_config_store.current().as_ref().clone();
            if cfg.iam.enforce != forced {
                cfg.iam.enforce = forced;
                if let Err(e) = runtime_config_store.apply(cfg) {
                    warn!(error = %e, "Failed to persist forced IAM enforcement flag");
                }
            }
            info!(enforce = forced, "IAM enforcement set from CLI/env");
        }
        authz.set_enabled(runtime_config_store.current().iam.enforce);
        let authz = Arc::clone(&authz);
        runtime_config_store.on_change(Box::new(move |cfg| {
            authz.set_enabled(cfg.iam.enforce);
            info!(enforce = cfg.iam.enforce, "IAM enforcement hot-reloaded");
        }));
    }

    // Log-level hot reload. Validation already ran in `apply()` so
    // the directive parses; on the off chance it fails here (e.g.
    // toctou with a removed feature flag), we keep the previous
    // filter rather than wedging logging.
    {
        // Apply the persisted level if it differs from the CLI seed
        // (e.g. user previously persisted "debug" via UI).
        let persisted = runtime_config_store.current().logging.level.clone();
        if persisted != cli.log_level
            && let Ok(filter) = tracing_subscriber::EnvFilter::try_new(&persisted)
        {
            let _ = log_reload.modify(|f| *f = filter);
        }
        runtime_config_store.on_change(Box::new(move |cfg| {
            match tracing_subscriber::EnvFilter::try_new(&cfg.logging.level) {
                Ok(filter) => {
                    if log_reload.modify(|f| *f = filter).is_ok() {
                        info!(level = %cfg.logging.level, "Log filter hot-reloaded");
                    }
                }
                Err(e) => warn!(error = %e, "Log filter rejected at reload time"),
            }
        }));
    }

    // Always-on signal handler that removes the DynamoDB tempdir
    // before exit. The richer save-snapshots-on-shutdown handler
    // below is gated on `--data-dir` and supersedes this one — it
    // takes care of the same tempdir cleanup in its exit path.
    if cli.data_dir.is_none() {
        let dynamodb_for_cleanup = Arc::clone(&dynamodb_service);
        let logs_for_cleanup = Arc::clone(&logs_service);
        let cw_metrics_for_cleanup = Arc::clone(&cw_metrics_service);
        let kinesis_for_cleanup = Arc::clone(&kinesis_service);
        let ses_for_cleanup = Arc::clone(&ses_service);
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT");
                let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM");
                tokio::select! {
                    _ = sigint.recv() => {}
                    _ = sigterm.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.ok();
            }
            cleanup_tempdir("DynamoDB", dynamodb_for_cleanup.tempdir_path());
            cleanup_tempdir("CloudWatch Logs", logs_for_cleanup.tempdir_path());
            cleanup_tempdir("CloudWatch Metrics", cw_metrics_for_cleanup.tempdir_path());
            cleanup_tempdir("Kinesis", kinesis_for_cleanup.tempdir_path());
            cleanup_tempdir("SES", ses_for_cleanup.tempdir_path());
            info!("Exiting.");
            std::process::exit(0);
        });
    }

    // STS session expiry sweep — drops expired entries from the
    // session store every 5 minutes so a long-running server doesn't
    // accumulate dead temp creds. `lookup` already filters expired
    // entries on the request path; this just keeps the map size
    // bounded between lookups.
    {
        let sessions = Arc::clone(&sts_sessions);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
            tick.tick().await; // skip first immediate tick
            loop {
                tick.tick().await;
                sessions.purge_expired();
            }
        });
    }

    // SES outbox retention sweep — drops emails older than
    // `ses.retention_hours` once per hour. The retention value is
    // read from the runtime config on every tick, so flipping it
    // from the UI takes effect on the next sweep without a restart.
    // A retention of 0 disables trimming for that tick.
    {
        let ses_for_sweep = Arc::clone(&ses_service);
        let cfg_store = Arc::clone(&runtime_config_store);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
            tick.tick().await; // skip first immediate tick
            loop {
                tick.tick().await;
                let retention_hours = cfg_store.current().ses.retention_hours;
                if retention_hours == 0 {
                    continue;
                }
                if let Some(store) = ses_for_sweep.sqlite_store_handle() {
                    let cutoff = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0)
                        - (retention_hours as i64) * 3600;
                    let store = Arc::clone(&store);
                    match tokio::task::spawn_blocking(move || store.trim_older_than(cutoff)).await {
                        Ok(Ok(removed)) if removed > 0 => {
                            info!(removed, retention_hours, "SES retention sweep")
                        }
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => warn!(error = %e.message, "SES retention sweep failed"),
                        Err(e) => warn!(error = %e, "SES retention sweep join error"),
                    }
                }
            }
        });
    }

    // Persistence: restore snapshots if --data-dir was provided.
    if let Some(ref data_dir) = cli.data_dir {
        let pm = PersistenceManager::new(data_dir);
        info!(data_dir = %data_dir, "Persistence enabled — restoring snapshots");
        pm.restore_all(&state.services);

        // Billing counters are persisted as a regular snapshot file but
        // sit outside the ServiceHandler map (billing isn't an AWS service).
        if let Some(bytes) = pm.load_snapshot("billing")
            && let Err(e) = billing_meter.store.restore_from_bytes(&bytes)
        {
            warn!(error = %e, "Failed to restore billing snapshot");
        }

        if let Some(bytes) = pm.load_snapshot("chaos")
            && let Err(e) = state.chaos.restore_from_bytes(&bytes)
        {
            warn!(error = %e, "Failed to restore chaos snapshot");
        }

        if !cli.no_gc {
            run_gc(
                s3_service.as_ref(),
                lambda_service.as_ref(),
                sqs_service.as_ref(),
                ecr_service.as_ref(),
            );
        }

        if let Some(secs) = cli.gc_interval_secs {
            let s3_gc = Arc::clone(&s3_service);
            let lambda_gc = Arc::clone(&lambda_service);
            let sqs_gc = Arc::clone(&sqs_service);
            let ecr_gc = Arc::clone(&ecr_service);
            let interval = std::time::Duration::from_secs(secs);
            info!(interval_secs = secs, "Periodic BodyStore GC enabled");
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;
                    run_gc(
                        s3_gc.as_ref(),
                        lambda_gc.as_ref(),
                        sqs_gc.as_ref(),
                        ecr_gc.as_ref(),
                    );
                }
            });
        }

        // Spawn graceful-shutdown handler that saves snapshots on SIGINT/SIGTERM.
        let services_for_shutdown = Arc::clone(&state.services);
        let pm_shutdown = Arc::new(PersistenceManager::new(data_dir));
        let billing_for_shutdown = Arc::clone(&billing_meter);
        let chaos_for_shutdown = Arc::clone(&state.chaos);
        let dynamodb_for_shutdown = Arc::clone(&dynamodb_service);
        let logs_for_shutdown = Arc::clone(&logs_service);
        let cw_metrics_for_shutdown = Arc::clone(&cw_metrics_service);
        let kinesis_for_shutdown = Arc::clone(&kinesis_service);
        let ses_for_shutdown = Arc::clone(&ses_service);
        let workers_for_shutdown = state.workers.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
                tokio::select! {
                    _ = sigint.recv() => {}
                    _ = sigterm.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.ok();
            }

            info!("Shutdown signal received - draining background workers + saving snapshots...");
            workers_for_shutdown
                .shutdown(awsim_core::tick::DEFAULT_SHUTDOWN_DEADLINE)
                .await;
            pm_shutdown.save_all(&services_for_shutdown);
            if let Some(bytes) = billing_for_shutdown.store.snapshot_to_bytes()
                && let Err(e) = pm_shutdown.save_snapshot("billing", &bytes)
            {
                warn!(error = %e, "Failed to save billing snapshot on shutdown");
            }
            if let Some(bytes) = chaos_for_shutdown.snapshot_to_bytes()
                && let Err(e) = pm_shutdown.save_snapshot("chaos", &bytes)
            {
                warn!(error = %e, "Failed to save chaos snapshot on shutdown");
            }
            cleanup_tempdir("DynamoDB", dynamodb_for_shutdown.tempdir_path());
            cleanup_tempdir("CloudWatch Logs", logs_for_shutdown.tempdir_path());
            cleanup_tempdir("CloudWatch Metrics", cw_metrics_for_shutdown.tempdir_path());
            cleanup_tempdir("Kinesis", kinesis_for_shutdown.tempdir_path());
            cleanup_tempdir("SES", ses_for_shutdown.tempdir_path());

            info!("Snapshots saved. Exiting.");
            std::process::exit(0);
        });

        // Spawn periodic auto-save every 30 seconds.
        let services_for_autosave = Arc::clone(&state.services);
        let pm_autosave = Arc::new(PersistenceManager::new(data_dir));
        let billing_for_autosave = Arc::clone(&billing_meter);
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(30);
            loop {
                tokio::time::sleep(interval).await;
                // `save_all` serialises every service's state to JSON
                // (potentially hundreds of MB after a bulk import) and
                // writes it to disk via blocking `std::fs::write`. If we
                // ran it on the async runtime it would freeze every
                // worker thread for the duration of the snapshot, so
                // requests would time out / error during each save.
                // `spawn_blocking` parks the work on the dedicated
                // blocking pool instead.
                let pm = Arc::clone(&pm_autosave);
                let services = Arc::clone(&services_for_autosave);
                let billing = Arc::clone(&billing_for_autosave);
                if let Err(e) = tokio::task::spawn_blocking(move || {
                    pm.save_all(&services);
                    if let Some(bytes) = billing.store.snapshot_to_bytes()
                        && let Err(e) = pm.save_snapshot("billing", &bytes)
                    {
                        warn!(error = %e, "Failed to save billing snapshot");
                    }
                })
                .await
                {
                    warn!(error = %e, "Snapshot save_all task panicked");
                }
            }
        });

        // Periodic point-in-time sampling task — drives both at-rest
        // storage metering (BodyStore/SQLite bytes × $/GB-mo) and
        // resource-hour metering (running EC2/RDS instances × $/hr).
        // Only runs in persistent mode — in-memory mode has no
        // on-disk size to query, but instance counts work either way
        // so we sample those regardless.
        let body_stores_for_storage = Arc::clone(&state.body_stores);
        let billing_for_storage = Arc::clone(&billing_meter);
        let data_dir_for_storage = data_dir.clone();
        let account_for_storage = cli.account_id.clone();
        let region_for_storage = cli.region.clone();
        let ec2_for_storage = Arc::clone(&ec2_service);
        let rds_for_storage = Arc::clone(&rds_service);
        let mq_for_storage = Arc::clone(&mq_service);
        let memorydb_for_storage = Arc::clone(&memorydb_service);
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(30);
            loop {
                tokio::time::sleep(interval).await;

                // Sum BodyStore bytes per service. Some services have
                // multiple groups (S3 stores objects under "s3"; Lambda
                // stores function code under "lambda" etc.).
                // The body-store handle uses display-style names; the
                // billing meter keys on signing names from the request
                // event stream, so remap any that diverge.
                let mut bytes_by_service: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                for handle in body_stores_for_storage.iter() {
                    let mut total: u64 = 0;
                    for group in &handle.groups {
                        total =
                            total.saturating_add(handle.body_store.group_size(group).unwrap_or(0));
                    }
                    let service_key = match handle.service_name.as_str() {
                        "cloudwatch-logs" => "logs".to_string(),
                        other => other.to_string(),
                    };
                    bytes_by_service.insert(service_key, total);
                }

                // DDB lives in a SQLite file rather than a BodyStore.
                let ddb_path = std::path::Path::new(&data_dir_for_storage).join("dynamodb.db");
                if let Ok(meta) = std::fs::metadata(&ddb_path) {
                    bytes_by_service.insert("dynamodb".to_string(), meta.len());
                }

                for (service, bytes) in bytes_by_service {
                    billing_for_storage.record_storage_sample(
                        &service,
                        &account_for_storage,
                        &region_for_storage,
                        bytes,
                    );
                }

                // Resource-hour billing: running instance counts.
                let ec2_count = ec2_for_storage
                    .running_instance_count(&account_for_storage, &region_for_storage);
                billing_for_storage.record_resource_count_sample(
                    "ec2",
                    &account_for_storage,
                    &region_for_storage,
                    ec2_count,
                );
                let rds_count = rds_for_storage
                    .running_instance_count(&account_for_storage, &region_for_storage);
                billing_for_storage.record_resource_count_sample(
                    "rds",
                    &account_for_storage,
                    &region_for_storage,
                    rds_count,
                );
                let mq_count =
                    mq_for_storage.running_broker_count(&account_for_storage, &region_for_storage);
                billing_for_storage.record_resource_count_sample(
                    "mq",
                    &account_for_storage,
                    &region_for_storage,
                    mq_count,
                );
                let memorydb_count = memorydb_for_storage
                    .running_node_count(&account_for_storage, &region_for_storage);
                billing_for_storage.record_resource_count_sample(
                    "memorydb",
                    &account_for_storage,
                    &region_for_storage,
                    memorydb_count,
                );
            }
        });
    }

    let service_count = state.services.len();

    // Spawn background event router — handles cross-service fan-out.
    spawn_event_router(&state);

    // Spawn the per-service tick loop. Each ServiceHandler::tick is
    // called once per interval; services use it for lifecycle work
    // that doesn't fit in the request path (DDB TTL, SQS
    // visibility-timeout reclamation, S3 lifecycle, EventBridge
    // scheduling, ...). The default trait impl is a no-op so
    // non-tick-aware services pay nothing.
    let tick_interval_ms = std::env::var("AWSIM_TICK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| (10..=60_000).contains(n))
        .unwrap_or(1000);
    awsim_core::gateway::spawn_tick_loop(
        state.clone(),
        std::time::Duration::from_millis(tick_interval_ms),
    );

    // Spawn SQS->Lambda poller: periodically polls SQS queues for event source mappings.
    let poll_services = Arc::clone(&state.services);
    let sqs_lambda_store = lambda_store.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            integrations::poll_sqs_event_sources(&poll_services, &sqs_lambda_store).await;
        }
    });

    // Spawn Kinesis->Lambda poller: periodically polls Kinesis streams for event source mappings.
    let kinesis_poll_services = Arc::clone(&state.services);
    let kinesis_lambda_store = lambda_store.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            integrations::poll_kinesis_event_sources(&kinesis_poll_services, &kinesis_lambda_store)
                .await;
        }
    });

    // Spawn EventBridge Pipes runner: forwards source records to targets for
    // every RUNNING pipe.
    let pipes_runner_services = Arc::clone(&state.services);
    let pipes_runner_store = pipes_store.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            integrations::pipes::run_pipes_once(&pipes_runner_services, &pipes_runner_store).await;
        }
    });

    // Build the API Gateway proxy state using the concrete Arc returned from register_services.
    let lambda_arc = state.services.get("lambda").cloned();

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(29)) // matches AWS API Gateway max
        .build()
        .expect("build reqwest client for HTTP integrations");
    let proxy_state = proxy::ProxyState {
        apigw: apigw_service,
        apigw_v1: apigw_v1_service,
        lambda: lambda_arc,
        http_client,
        default_account_id: cli.account_id.clone(),
        default_region: cli.region.clone(),
    };

    // Build the proxy sub-router (finalized with its own state).
    // Proxy routes are scoped under the literal `_user_request_` segment so
    // they never shadow the v1 management API (`/restapis/{id}/resources/...`,
    // `/restapis/{id}/authorizers`, etc.). Three variants handle the bare
    // path (no slash), the trailing-slash root, and any nested path.
    let proxy_router: axum::Router<()> = axum::Router::new()
        .route(
            "/restapis/{api_id}/{stage}/_user_request_",
            axum::routing::any(proxy::handle_proxy),
        )
        .route(
            "/restapis/{api_id}/{stage}/_user_request_/",
            axum::routing::any(proxy::handle_proxy),
        )
        .route(
            "/restapis/{api_id}/{stage}/_user_request_/{*path}",
            axum::routing::any(proxy::handle_proxy),
        )
        .with_state(proxy_state);

    // Build the Cognito OAuth/OIDC sub-router (standard HTTP, no SigV4).
    // `cognito_state` is an Arc<CognitoState> for the default account+region,
    // shared with the CognitoService so OAuth and API calls see the same pools.
    let cognito_oauth_state = Arc::new(awsim_cognito::CognitoOAuthState {
        cognito: Arc::clone(&cognito_state),
        default_account_id: cli.account_id.clone(),
        default_region: cli.region.clone(),
        auth_codes: Arc::new(dashmap::DashMap::new()),
        revoked_refresh_tokens: Arc::new(dashmap::DashMap::new()),
        federation: awsim_cognito::federation::FederationState::new(),
        port: cli.port,
    });
    let cognito_oauth_router = awsim_cognito::oauth::router(cognito_oauth_state);

    // Built-in mock OIDC identity provider for offline federation
    // testing. Self-hosted under `/_awsim/idp/{provider_id}/...`; a
    // Cognito IdentityProvider of type `OIDC` whose ProviderDetails
    // point at the discovery URL gets the full federation flow
    // working without any external network calls.
    let mock_idp_state = awsim_cognito::mock_idp::MockIdpState::new();
    let mock_idp_router = awsim_cognito::mock_idp::router(mock_idp_state);

    // Set up TLS *before* the routers so the `/_awsim/tls` admin
    // route can hand the cert path to bootstrap tooling, and so a
    // bogus `--tls-cert` path / unwritable cache dir fails fast
    // before we do all the heavy state init.
    let https_runtime = if let Some(https_port) = cli.https_port {
        Some(prepare_https_runtime(&cli, https_port).await?)
    } else {
        None
    };
    let tls_admin_info: Option<Arc<tls::TlsAdminInfo>> = https_runtime
        .as_ref()
        .map(|h| Arc::new(h.assets.admin_info(h.port)));

    // Build the main router (finalized with AppState).
    //
    // S3 upload routes (PutObject path-style and UploadPart) get a
    // dedicated route with a much larger body cap so single-object PUTs
    // up to 5 GiB work without forcing every other endpoint to inherit
    // that headroom. The pattern matches `/{bucket}/{key+}` PUTs that
    // either AWS SDKs use directly or that point at multipart upload
    // (which the gateway distinguishes via the `uploadId` query string
    // before dispatching). Other methods on the same path fall through
    // to the universal gateway with the smaller `--max-body-bytes` cap.
    let s3_upload_limit = cli.max_s3_upload_bytes;
    let operator_auth_state = operator_auth::OperatorAuthState::new(
        Arc::clone(&iam_service),
        cli.account_id.clone(),
        cli.region.clone(),
    );
    let auth_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/auth/login",
            axum::routing::post(operator_auth::login),
        )
        .route(
            "/_awsim/auth/logout",
            axum::routing::post(operator_auth::logout),
        )
        .route(
            "/_awsim/auth/whoami",
            axum::routing::get(operator_auth::whoami),
        )
        .with_state(operator_auth_state);
    let main_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/health", axum::routing::get(admin::health))
        .route("/_awsim/services", axum::routing::get(admin::list_services))
        .route("/_awsim/config", axum::routing::get(admin::config))
        .route("/_awsim/stats", axum::routing::get(admin::stats))
        .route("/_awsim/storage", axum::routing::get(admin::storage))
        .route("/_awsim/events", axum::routing::get(admin::events))
        .route(
            "/_awsim/requests",
            axum::routing::get(admin::recent_request_ids),
        )
        .route(
            "/_awsim/requests/{id}",
            axum::routing::get(admin::request_detail),
        )
        .route(
            "/_awsim/requests/{id}/replay",
            axum::routing::post(admin::replay_request),
        )
        .layer(axum::middleware::from_fn(operator_auth::require_auth))
        // Wide-open catch-all on `/{bucket}/{*key}`. Hands every
        // method to the gateway (which routes to the right service
        // by inspecting headers / path / verb internally) and
        // applies the larger body cap so S3 PutObject / UploadPart
        // up to 5 GiB work. Earlier versions registered only PUT
        // here, but axum's MethodRouter returns a 405
        // `Allow: PUT` for any other method on a registered path -
        // it does NOT fall through to `.fallback(...)` - which
        // silently broke every multi-segment AWS REST endpoint
        // (`GET /v1/apps`, `GET /v2/email/identities`, S3
        // GetObject, Lambda Invoke, ...) the moment they were
        // requested via something other than PUT. The looser cap
        // on non-PUT methods is fine: SDK-generated GET / DELETE /
        // POST bodies are naturally small, and the cap is a
        // streaming upper bound, not a forced allocation.
        .route(
            "/{bucket}/{*key}",
            axum::routing::any(awsim_core::gateway::handle_request)
                .layer(axum::extract::DefaultBodyLimit::max(s3_upload_limit)),
        )
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state.clone())
        .merge(auth_router);

    // Build the OpenSearch (Elasticsearch-compatible) sub-router.
    // Nest under `/opensearch` so it doesn't collide with AWS routes.
    //
    // Storage is redb-backed (per-doc key-value), so the working set
    // is bounded by disk rather than RAM and writes are durable on
    // commit — no snapshot save/restore loop needed. With `--data-dir`
    // we put the database alongside other service snapshots; without
    // it we fall back to a tempdir for ephemeral runs.
    let opensearch_state = match cli.data_dir.as_deref() {
        Some(dir) => {
            let path = std::path::Path::new(dir).join("opensearch.redb");
            Arc::new(
                awsim_opensearch::state::OpenSearchState::open(&path)
                    .expect("Failed to open opensearch redb"),
            )
        }
        None => Arc::new(
            awsim_opensearch::state::OpenSearchState::ephemeral()
                .expect("Failed to create ephemeral opensearch state"),
        ),
    };
    let opensearch_nested: axum::Router<()> =
        axum::Router::new().nest("/opensearch", awsim_opensearch::router(opensearch_state));

    let ecr_router = awsim_ecr::router(ecr_service);

    // Billing sub-router. Carries its own state (Arc<BillingMeter>) so it
    // doesn't have to be plumbed through AppState — keeps awsim-core free
    // of billing concerns.
    let billing_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/billing", axum::routing::get(admin::billing))
        .with_state(Arc::clone(&billing_meter));

    // Chaos sub-router. Same pattern as billing — its own typed
    // state so the admin handlers can mutate the engine without
    // routing everything through AppState.
    let chaos_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/chaos/rules",
            axum::routing::get(admin::chaos_list).post(admin::chaos_add),
        )
        .route(
            "/_awsim/chaos/rules/{id}",
            axum::routing::patch(admin::chaos_patch).delete(admin::chaos_remove),
        )
        .route(
            "/_awsim/chaos/clear",
            axum::routing::post(admin::chaos_clear),
        )
        .route(
            "/_awsim/chaos/stats",
            axum::routing::get(admin::chaos_stats),
        )
        .route(
            "/_awsim/chaos/presets",
            axum::routing::get(admin::chaos_presets_list),
        )
        .route(
            "/_awsim/chaos/presets/{name}",
            axum::routing::post(admin::chaos_preset_apply),
        )
        .with_state(Arc::clone(&state.chaos));

    // DynamoDB admin sub-router. Carries an Arc<DynamoDbService> so
    // we can run VACUUM without going through the gateway protocol path.
    let ddb_admin_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/admin/dynamodb/vacuum",
            axum::routing::post(admin::ddb_vacuum),
        )
        .with_state(Arc::clone(&dynamodb_service));

    // SQLite-backed storage stats endpoint. Surfaces row counts +
    // db file sizes for the four high-volume services so users can
    // see where memory / disk is going.
    let sqlite_stats_state = Arc::new(admin::SqliteStatsState {
        dynamodb: Arc::clone(&dynamodb_service),
        cw_logs: Arc::clone(&logs_service),
        cw_metrics: Arc::clone(&cw_metrics_service),
        kinesis: Arc::clone(&kinesis_service),
        ses: Arc::clone(&ses_service),
    });
    let sqlite_stats_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/storage/sqlite",
            axum::routing::get(admin::sqlite_stats),
        )
        .with_state(Arc::clone(&sqlite_stats_state));

    // SES outbox inspector. Lets the admin UI list every captured
    // outbound email across all accounts/regions.
    let ses_admin_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/ses/sent", axum::routing::get(admin::ses_sent))
        .with_state(Arc::clone(&ses_service));

    // Memory diagnostic. Aggregates counts across every major
    // in-memory store so users can diff snapshots and pinpoint
    // what's growing.
    let debug_objects_state = Arc::new(admin::DebugObjectsState {
        app: state.clone(),
        billing: Arc::clone(&billing_meter),
        cognito: Arc::clone(&cognito_state),
        sqlite: Arc::clone(&sqlite_stats_state),
    });
    // Bulk-seed router. Each /_awsim/seed/<service> writes directly
    // to the service's internal state — no SigV4, no per-request
    // overhead — so a 10k-row seed lands in well under a second.
    let seed_ddb_state = Arc::new(seed::dynamodb::SeedDdbState {
        service: Arc::clone(&dynamodb_service),
        default_account: cli.account_id.clone(),
        default_region: cli.region.clone(),
    });
    let seed_cognito_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/seed/cognito-users",
            axum::routing::post(seed::cognito::seed),
        )
        .with_state(Arc::clone(&cognito_state));
    let seed_ddb_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/seed/dynamodb",
            axum::routing::post(seed::dynamodb::seed),
        )
        .with_state(seed_ddb_state);
    let seed_s3_state = Arc::new(seed::s3::SeedS3State {
        service: Arc::clone(&s3_service),
        default_account: cli.account_id.clone(),
        default_region: cli.region.clone(),
    });
    let seed_s3_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/seed/s3", axum::routing::post(seed::s3::seed))
        .with_state(seed_s3_state);
    let seed_secrets_state = Arc::new(seed::secrets::SeedSecretsState {
        store: secrets_store.clone(),
        default_account: cli.account_id.clone(),
        default_region: cli.region.clone(),
    });
    let seed_secrets_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/seed/secrets",
            axum::routing::post(seed::secrets::seed),
        )
        .with_state(seed_secrets_state);
    let seed_sqs_state = Arc::new(seed::sqs::SeedSqsState {
        store: sqs_store.clone(),
        default_account: cli.account_id.clone(),
        default_region: cli.region.clone(),
        default_port: cli.port,
    });
    let seed_sqs_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/seed/sqs", axum::routing::post(seed::sqs::seed))
        .with_state(seed_sqs_state);
    let seed_router = seed_cognito_router
        .merge(seed_ddb_router)
        .merge(seed_s3_router)
        .merge(seed_secrets_router)
        .merge(seed_sqs_router);

    let debug_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/debug/objects",
            axum::routing::get(admin::debug_objects),
        )
        .with_state(debug_objects_state);

    let bedrock_admin_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/bedrock/config",
            axum::routing::get(admin::bedrock_config),
        )
        .route(
            "/_awsim/bedrock/backends/{name}/check",
            axum::routing::get(admin::bedrock_backend_check),
        )
        .with_state(Arc::clone(&bedrock_swap));

    let bedrock_defaults_router: axum::Router<()> = axum::Router::new().route(
        "/_awsim/bedrock/defaults",
        axum::routing::get(admin::bedrock_defaults),
    );

    let gateway_catalog_router: axum::Router<()> = axum::Router::new().route(
        "/_awsim/gateway/catalog",
        axum::routing::get(admin::gateway_catalog),
    );

    let gateway_health_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/gateway/health",
            axum::routing::get(admin::gateway_health),
        )
        .with_state(bedrock_health.clone());

    let gateway_health_check_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/gateway/health/{name}/check",
            axum::routing::post(admin::gateway_health_check),
        )
        .with_state((Arc::clone(&bedrock_swap), bedrock_health.clone()));

    let gateway_metrics_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/gateway/metrics",
            axum::routing::get(admin::gateway_metrics),
        )
        .with_state(bedrock_metrics.clone());

    let gateway_recent_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/gateway/recent",
            axum::routing::get(admin::gateway_recent),
        )
        .with_state(bedrock_recent.clone());

    let gateway_test_prompt_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/gateway/test-prompt",
            axum::routing::post(admin::gateway_test_prompt),
        )
        .with_state(Arc::clone(&bedrock_swap));

    let runtime_config_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/runtime-config",
            axum::routing::get(admin::runtime_config_get).put(admin::runtime_config_put),
        )
        .route(
            "/_awsim/runtime-config/defaults",
            axum::routing::get(admin::runtime_config_defaults),
        )
        .with_state(Arc::clone(&runtime_config_store));

    // Named-snapshot sub-router. Bundles ServiceHandler state +
    // billing + chaos under `{data_dir}/named-snapshots/{name}/`.
    let snapshot_state = Arc::new(named_snapshots::SnapshotState {
        app: state.clone(),
        billing: Arc::clone(&billing_meter),
    });
    let snapshot_router: axum::Router<()> = axum::Router::new()
        .route(
            "/_awsim/snapshots",
            axum::routing::get(named_snapshots::list),
        )
        .route(
            "/_awsim/snapshots/{name}",
            axum::routing::post(named_snapshots::save).delete(named_snapshots::delete),
        )
        .route(
            "/_awsim/snapshots/{name}/load",
            axum::routing::post(named_snapshots::load),
        )
        .with_state(snapshot_state);

    // Surfaces the active HTTPS port + on-disk cert path so
    // bootstrap tooling can wire up `NODE_EXTRA_CA_CERTS` (or any
    // other client trust knob) without out-of-band knowledge of the
    // awsim install. Returns 404 when HTTPS is off.
    let tls_admin_router: axum::Router<()> = axum::Router::new()
        .route("/_awsim/tls", axum::routing::get(admin::tls_info))
        .with_state(tls_admin_info.clone());

    // Merge all routers and add shared middleware.
    let app = cognito_oauth_router
        .merge(mock_idp_router)
        .merge(main_router)
        .merge(proxy_router)
        .merge(opensearch_nested)
        .merge(ecr_router)
        .merge(billing_router)
        .merge(chaos_router)
        .merge(snapshot_router)
        .merge(ddb_admin_router)
        .merge(sqlite_stats_router)
        .merge(ses_admin_router)
        .merge(debug_router)
        .merge(bedrock_admin_router)
        .merge(bedrock_defaults_router)
        .merge(gateway_catalog_router)
        .merge(gateway_health_router)
        .merge(gateway_health_check_router)
        .merge(gateway_metrics_router)
        .merge(gateway_recent_router)
        .merge(gateway_test_prompt_router)
        .merge(runtime_config_router)
        .merge(seed_router)
        .merge(tls_admin_router)
        .merge(ui::router())
        // Redirect plain browser hits on `/` to the admin UI so users
        // don't have to remember the `/_awsim/ui/` path. Skips when the
        // request looks like an AWS SDK call (SigV4 Authorization +
        // non-HTML Accept).
        .layer(axum::middleware::from_fn(ui::root_redirect_middleware))
        // Transparent request-body decompression. AWS SDKs that opt
        // into compression (OpenSearch JS client with
        // `compression: gzip`, CloudWatch Logs PutLogEvents, etc.)
        // ship gzip-encoded bodies; without this layer the inner
        // handlers see raw gzip bytes and 400 with an opaque error.
        // Layered *after* DefaultBodyLimit so the limit applies to
        // the decompressed payload rather than the wire bytes - the
        // handler-visible body is what the user-facing cap should
        // gate. tower-http strips `Content-Encoding` + `Content-Length`
        // for the inner service.
        .layer(tower_http::decompression::RequestDecompressionLayer::new())
        .layer(axum::extract::DefaultBodyLimit::max(cli.max_body_bytes))
        // Bounded in-flight requests with shed-on-overload. A misbehaving
        // client (leaking sockets during a bulk import, hammering with
        // unbounded parallelism) can't accumulate work past the cap —
        // excess requests get an immediate 503 instead of queueing
        // indefinitely and starving the runtime / exhausting fds.
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_overload_error))
                .layer(LoadShedLayer::new())
                .layer(ConcurrencyLimitLayer::new(cli.max_concurrent_requests)),
        )
        .layer(tower_http::cors::CorsLayer::permissive());

    info!(
        max_concurrent_requests = cli.max_concurrent_requests,
        "Inflight-request cap enabled"
    );

    spawn_fd_pressure_watcher();

    let listener = bind_dual_stack_tokio(cli.port).await?;

    // Startup banner
    println!();
    println!("  AWSim v{}", env!("CARGO_PKG_VERSION"));
    println!("  Fully Offline AWS Dev Environment");
    println!();
    println!("  Endpoint:  http://localhost:{}", cli.port);
    if let Some(ref tls) = https_runtime {
        let host = tls.assets.domain.as_deref().unwrap_or("localhost");
        println!("  HTTPS:     https://{}:{}", host, tls.port);
        if tls.assets.public_trust {
            println!(
                "             (publicly-trusted cert - no AWS_CA_BUNDLE / NODE_EXTRA_CA_CERTS needed)"
            );
        } else {
            println!(
                "             export AWS_CA_BUNDLE={}",
                tls.assets.cert_path.display()
            );
            if tls.assets.generated {
                println!("             (self-signed cert generated; reused on subsequent boots)");
            }
        }
    }
    if ui::is_bundled() {
        println!("  Admin UI:  http://localhost:{}/_awsim/ui/", cli.port);
    }
    println!("  Region:    {}", cli.region);
    println!("  Account:   {}", cli.account_id);
    println!("  Services:  {} registered", service_count);
    if let Some(ref data_dir) = cli.data_dir {
        println!("  Persist:   {}", data_dir);
    }
    println!();

    info!(
        port = cli.port,
        https_port = ?cli.https_port,
        region = %cli.region,
        account_id = %cli.account_id,
        services = service_count,
        "AWSim started"
    );

    match https_runtime {
        Some(tls) => {
            // Both listeners share the same `Router`. `Router` is
            // `Clone` (Arc-backed internally), so cloning is cheap.
            // The HTTPS clone gets an extra layer that injects
            // `X-Forwarded-Proto: https` so handlers (and the
            // gateway's existing forwarded-proto check) see the
            // request as secure - matters for any URL builder that
            // emits absolute URLs reflecting the request scheme
            // (e.g. Cognito's OIDC discovery doc).
            let http_app = app.clone();
            let https_app = app.layer(axum::middleware::from_fn(mark_request_https));
            let http_fut = async move {
                axum::serve(listener, http_app)
                    .await
                    .context("HTTP listener failed")
            };
            let https_fut = async move {
                axum_server::from_tcp_rustls(tls.std_listener, tls.assets.config)
                    .serve(https_app.into_make_service())
                    .await
                    .context("HTTPS listener failed")
            };
            tokio::try_join!(http_fut, https_fut)?;
        }
        None => {
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

struct HttpsRuntime {
    port: u16,
    assets: tls::TlsAssets,
    std_listener: std::net::TcpListener,
}

/// Build the TLS material + bound socket for the HTTPS listener.
///
/// Selection precedence:
///   1. BYO (`--tls-cert` + `--tls-key`).
///   2. Bundled publicly-trusted cert (when this binary was built
///      with the `aws.qaidvoid.dev` assets in place).
///   3. Self-signed managed cert under `--tls-cache-dir` (default
///      `<data-dir>/tls` or `$XDG_CACHE_HOME/awsim/tls`).
async fn prepare_https_runtime(cli: &Cli, https_port: u16) -> Result<HttpsRuntime> {
    let source = match (cli.tls_cert.as_deref(), cli.tls_key.as_deref()) {
        (Some(cert), Some(key)) => tls::CertSource::Byo { cert, key },
        _ => {
            let dir = cli
                .tls_cache_dir
                .clone()
                .or_else(|| {
                    cli.data_dir
                        .as_deref()
                        .map(|d| std::path::PathBuf::from(d).join("tls"))
                })
                .unwrap_or_else(tls::default_cache_dir);
            #[cfg(has_bundled_cert)]
            {
                tls::CertSource::Bundled { dir }
            }
            #[cfg(not(has_bundled_cert))]
            {
                tls::CertSource::Managed { dir }
            }
        }
    };

    let assets = tls::load_or_generate(source).await?;
    let std_listener = bind_dual_stack_std(https_port)
        .with_context(|| format!("binding HTTPS listener on port {https_port}"))?;
    Ok(HttpsRuntime {
        port: https_port,
        assets,
        std_listener,
    })
}

/// Stamp `X-Forwarded-Proto: https` on every request that arrives
/// via the HTTPS listener so downstream handlers (and the gateway's
/// SigV4-side forwarded-proto check) can tell the request was
/// secure. Existing values win - upstream proxies are authoritative.
///
/// Also materialises the `Host` header from the URI's `:authority`
/// pseudo-header when absent. axum-server + rustls negotiates HTTP/2
/// via ALPN, and HTTP/2 omits `Host` in favour of `:authority`. Our
/// URL-builder code paths (Cognito issuer, SQS queue URLs, ...) read
/// the `Host` header uniformly, so we copy authority across to keep
/// them protocol-version-agnostic.
async fn mark_request_https(
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let authority = req.uri().authority().map(|a| a.to_string());
    let headers = req.headers_mut();
    if !headers.contains_key("x-forwarded-proto") {
        headers.insert(
            "x-forwarded-proto",
            axum::http::HeaderValue::from_static("https"),
        );
    }
    if !headers.contains_key("host")
        && let Some(a) = authority
        && let Ok(v) = axum::http::HeaderValue::from_str(&a)
    {
        headers.insert("host", v);
    }
    next.run(req).await
}

/// Bind a `tokio::net::TcpListener` on `[::]:port` with a fall back
/// to `0.0.0.0:port`.
///
/// Node 20+ resolves `localhost` to `::1` first, so an IPv4-only
/// bind on `0.0.0.0` makes the SDK fail with ECONNREFUSED before any
/// request ever reaches us. If the OS has `net.ipv6.bindv6only=1`
/// (uncommon on dev machines) or IPv6 is disabled, fall back to
/// plain v4.
async fn bind_dual_stack_tokio(port: u16) -> Result<tokio::net::TcpListener> {
    let addr_v6 = std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, port));
    match tokio::net::TcpListener::bind(addr_v6).await {
        Ok(l) => Ok(l),
        Err(e) => {
            warn!(port, error = %e, "Could not bind on [::] - falling back to 0.0.0.0");
            let addr_v4 = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            Ok(tokio::net::TcpListener::bind(addr_v4).await?)
        }
    }
}

/// Same dual-stack bind as `bind_dual_stack_tokio`, but returns a
/// `std::net::TcpListener` (set non-blocking) so it can be handed to
/// `axum_server::from_tcp_rustls`.
fn bind_dual_stack_std(port: u16) -> Result<std::net::TcpListener> {
    let addr_v6 = std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, port));
    let listener = match std::net::TcpListener::bind(addr_v6) {
        Ok(l) => l,
        Err(e) => {
            warn!(port, error = %e, "Could not bind on [::] - falling back to 0.0.0.0");
            let addr_v4 = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            std::net::TcpListener::bind(addr_v4)?
        }
    };
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Spawn a background task that consumes from the internal event bus and
/// performs cross-service fan-out deliveries.
///
/// Handles:
///   sns:Publish                    → sqs  — enqueues the SNS message body into the target queue
///   sns:Publish                    → lambda — (future) invokes the target Lambda function
/// Map tower errors to HTTP responses. The only error we expect from the
/// LoadShed + ConcurrencyLimit stack is `tower::load_shed::error::Overloaded`
/// — convert it to a friendly 503 with a hint. Anything else is unexpected
/// and surfaces as 500.
/// `process::exit` skips Drop, so any service that owns a tempdir
/// (no `--data-dir` case) wouldn't get its files cleaned up
/// automatically. Remove the directory by hand instead.
fn cleanup_tempdir(label: &str, path: Option<&std::path::Path>) {
    let Some(path) = path else { return };
    if let Err(e) = std::fs::remove_dir_all(path) {
        warn!(
            service = label,
            path = %path.display(),
            error = %e,
            "Failed to remove tempdir on shutdown"
        );
    }
}

/// One-shot client for `awsim vacuum` — calls the admin endpoint
/// on a running awsim instance.
async fn run_vacuum(endpoint: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let url = format!(
        "{}/_awsim/admin/dynamodb/vacuum",
        endpoint.trim_end_matches('/')
    );
    let resp = client.post(&url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {status}: {text}");
    }
    println!("DynamoDB VACUUM complete");
    Ok(())
}

async fn handle_overload_error(err: BoxError) -> impl IntoResponse {
    if err.is::<tower::load_shed::error::Overloaded>() {
        warn!("Request rejected — concurrency limit reached");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "AWSim is at the configured concurrent-request cap. \
             Bound your client's parallelism or raise --max-concurrent-requests.",
        )
            .into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled middleware error: {err}"),
        )
            .into_response()
    }
}

/// Background task that polls the process's open-fd count and warns when
/// the soft limit is being approached. Catches the runaway-connection
/// pattern (client leaks sockets, fds creep up) before the OS slams the
/// listener with EMFILE / ENFILE — gives the user a chance to spot it
/// in the logs and tune the client.
///
/// Linux-only (reads /proc/self/fd). On non-Linux it gracefully exits
/// after the first read failure.
#[cfg(unix)]
fn spawn_fd_pressure_watcher() {
    let pid = std::process::id();
    let fd_dir = std::path::PathBuf::from(format!("/proc/{pid}/fd"));
    if !fd_dir.exists() {
        debug!("/proc/<pid>/fd not available — skipping fd-pressure watcher");
        return;
    }
    let (_, hard) = match rlimit::getrlimit(rlimit::Resource::NOFILE) {
        Ok(p) => p,
        Err(_) => return,
    };
    let warn_at = (hard as f64 * 0.5) as u64;
    let crit_at = (hard as f64 * 0.8) as u64;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let count = match std::fs::read_dir(&fd_dir) {
                Ok(d) => d.count() as u64,
                Err(_) => break, // fs went away (proc unmounted? rare) — stop watching
            };
            if count >= crit_at {
                error!(
                    open_fds = count,
                    hard_limit = hard,
                    threshold_pct = 80,
                    "fd usage critical — listener will start dropping connections"
                );
            } else if count >= warn_at {
                warn!(
                    open_fds = count,
                    hard_limit = hard,
                    threshold_pct = 50,
                    "fd usage elevated — check for client connection leaks"
                );
            } else {
                debug!(open_fds = count, hard_limit = hard);
            }
        }
    });
}

/// Bump the open-files soft limit toward the hard limit (capped at 65,536).
///
/// Default Linux distros ship a 1024 soft limit, which a heavy bulk-import
/// workload (millions of rows × parallel connections × per-row writes)
/// blows through in seconds — leaving axum logging "Too many open files"
/// on every accept. Raising the soft limit at startup means users don't
/// have to remember to `ulimit -n` before launching the binary.
///
/// No-op on Windows (the rlimit crate's NOFILE doesn't exist there).
#[cfg(unix)]
fn raise_nofile_limit() {
    const TARGET: u64 = 65_536;
    let (soft, hard) = match rlimit::getrlimit(rlimit::Resource::NOFILE) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "Could not read NOFILE rlimit; skipping bump");
            return;
        }
    };
    let desired = TARGET.min(hard);
    if soft >= desired {
        return;
    }
    if let Err(e) = rlimit::setrlimit(rlimit::Resource::NOFILE, desired, hard) {
        warn!(
            from = soft,
            to = desired,
            hard = hard,
            error = %e,
            "Could not raise NOFILE rlimit; bulk imports may hit fd exhaustion",
        );
        return;
    }
    info!(
        from = soft,
        to = desired,
        hard = hard,
        "Raised NOFILE rlimit"
    );
}

#[cfg(not(unix))]
fn spawn_fd_pressure_watcher() {}

#[cfg(not(unix))]
fn raise_nofile_limit() {}

///   cloudformation:CreateResource  — provisions the resource in the target service
///   cloudformation:DeleteResource  — deprovisions the resource from the target service
fn spawn_event_router(state: &AppState) {
    use std::sync::Arc;

    let mut rx = state.event_bus.subscribe();
    let services = Arc::clone(&state.services);
    let default_region = state.default_region.clone();
    let default_account_id = state.default_account_id.clone();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let protocol = event.detail["protocol"].as_str().unwrap_or("").to_string();
                    let endpoint = event.detail["endpoint"].as_str().unwrap_or("").to_string();
                    let message = event.detail["message"].as_str().unwrap_or("").to_string();
                    let message_id = event.detail["message_id"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let topic_arn = event.detail["topic_arn"].as_str().unwrap_or("").to_string();
                    let subject = event.detail["subject"].as_str().map(str::to_string);
                    let subscription_arn = event.detail["subscription_arn"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let message_attributes = event.detail.get("message_attributes").cloned();
                    let raw_message_delivery = event.detail["raw_message_delivery"]
                        .as_bool()
                        .unwrap_or(false);

                    match event.event_type.as_str() {
                        "sns:Publish" if protocol == "sqs" => {
                            // endpoint is a queue ARN: arn:aws:sqs:{region}:{account}:{queue-name}
                            // Derive the queue URL so SQS SendMessage can find the queue.
                            let queue_url =
                                arn_to_sqs_url(&endpoint, &default_region, &default_account_id);

                            if let Some(sqs_handler) = services.get("sqs") {
                                // Build a minimal context (no event bus needed for delivery calls).
                                let ctx = RequestContext {
                                    account_id: event.account_id.clone(),
                                    region: event.region.clone(),
                                    partition: awsim_core::DEFAULT_PARTITION.to_string(),
                                    service: "sqs".to_string(),
                                    access_key: None,
                                    request_id: uuid::Uuid::new_v4().to_string(),
                                    method: "POST".to_string(),
                                    uri: "/".to_string(),
                                    event_bus: None,
                                    source_ip: None,
                                    is_secure: false,
                                };

                                // RawMessageDelivery=true subscriptions skip the SNS
                                // notification envelope entirely; the SQS body is the
                                // raw publish payload, and SNS publish-time message
                                // attributes flow through as native SQS message
                                // attributes (Type/Value form).
                                let mut input = if raw_message_delivery {
                                    let mut sqs_attrs = serde_json::Map::new();
                                    if let Some(attrs) = &message_attributes
                                        && let Some(map) = attrs.as_object()
                                    {
                                        for (k, v) in map {
                                            // SNS attribute envelope is { "Type", "Value" };
                                            // SQS uses { "DataType", "StringValue" }.
                                            let data_type = v["Type"].as_str().unwrap_or("String");
                                            let string_value = v["Value"].as_str().unwrap_or("");
                                            sqs_attrs.insert(
                                                k.clone(),
                                                serde_json::json!({
                                                    "DataType": data_type,
                                                    "StringValue": string_value,
                                                }),
                                            );
                                        }
                                    }
                                    let mut input = serde_json::json!({
                                        "QueueUrl": queue_url,
                                        "MessageBody": message,
                                    });
                                    if !sqs_attrs.is_empty() {
                                        input["MessageAttributes"] =
                                            serde_json::Value::Object(sqs_attrs);
                                    }
                                    input
                                } else {
                                    // Wrap the SNS message in the SNS notification envelope that
                                    // real AWS delivers to SQS subscribers. The fixture
                                    // signature/cert URL/timestamp are stable per-message but not
                                    // cryptographically real — clients that verify the signature
                                    // (rare in test environments) will fail; clients that just
                                    // read the metadata (the common case) round-trip cleanly.
                                    let timestamp = iso8601_now();
                                    let unsubscribe_url = format!(
                                        "http://sns.{region}.localhost/?Action=Unsubscribe&SubscriptionArn={sub}",
                                        region = event.region,
                                        sub = subscription_arn,
                                    );
                                    let signing_cert_url = format!(
                                        "http://sns.{region}.localhost/SimpleNotificationService-awsim.pem",
                                        region = event.region,
                                    );

                                    let mut envelope = serde_json::json!({
                                        "Type": "Notification",
                                        "MessageId": message_id,
                                        "TopicArn": topic_arn,
                                        "Message": message,
                                        "Timestamp": timestamp,
                                        "SignatureVersion": "1",
                                        "Signature": "awsim-fixture-signature",
                                        "SigningCertURL": signing_cert_url,
                                        "UnsubscribeURL": unsubscribe_url,
                                    });
                                    if let Some(s) = &subject {
                                        envelope["Subject"] = serde_json::Value::String(s.clone());
                                    }
                                    if let Some(attrs) = &message_attributes
                                        && attrs.as_object().is_some_and(|m| !m.is_empty())
                                    {
                                        envelope["MessageAttributes"] = attrs.clone();
                                    }
                                    serde_json::json!({
                                        "QueueUrl": queue_url,
                                        "MessageBody": envelope.to_string(),
                                    })
                                };
                                // Silence the read-only `let mut` lint when the
                                // raw-delivery branch produces no further mutations.
                                let _ = &mut input;

                                match sqs_handler.handle("SendMessage", input, &ctx).await {
                                    Ok(_) => {
                                        info!(
                                            topic = %topic_arn,
                                            queue = %endpoint,
                                            "SNS→SQS fan-out delivered"
                                        );
                                    }
                                    Err(e) => {
                                        warn!(
                                            topic = %topic_arn,
                                            queue = %endpoint,
                                            error = %e.message,
                                            "SNS→SQS fan-out delivery failed"
                                        );
                                    }
                                }
                            }
                        }
                        "sns:Publish" if protocol == "lambda" => {
                            // Lambda fan-out — reserved for future implementation.
                            info!(
                                topic = %topic_arn,
                                function = %endpoint,
                                "SNS→Lambda fan-out: not yet implemented"
                            );
                        }
                        "cloudformation:CreateResource" => {
                            integrations::handle_cf_create_resource(&services, &event).await;
                        }
                        "cloudformation:DeleteResource" => {
                            integrations::handle_cf_delete_resource(&services, &event).await;
                        }
                        "dynamodb:StreamRecord" => {
                            integrations::handle_dynamodb_stream(&services, &event).await;
                        }
                        "eventbridge:TargetInvocation" => {
                            integrations::handle_eventbridge_target(&services, &event).await;
                        }
                        "cognito:LambdaTrigger" => {
                            integrations::handle_cognito_trigger(&services, &event).await;
                        }
                        t if t.starts_with("s3:ObjectCreated:")
                            || t.starts_with("s3:ObjectRemoved:") =>
                        {
                            integrations::handle_s3_event(&services, &event).await;
                        }
                        _ => {
                            // Unknown or unhandled event type — ignore.
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        skipped = n,
                        "Event bus receiver lagged; some events were dropped"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Sender dropped — bus is shut down, exit the task.
                    break;
                }
            }
        }
    });
}

/// Convert a SQS queue ARN to its local URL.
///
/// ARN format:  `arn:aws:sqs:{region}:{account}:{queue-name}`
/// URL format:  `http://sqs.{region}.localhost:4566/{account}/{queue-name}`
///
/// Falls back to a best-effort URL using the supplied defaults when the ARN
/// cannot be parsed.
/// Format the current UTC time as the ISO 8601 string AWS uses on SNS
/// notification envelopes (e.g. `2026-05-04T09:00:00.000Z`).
fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let mut days = hours / 24;
    let mut y = 1970u64;
    loop {
        let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
        let dy = if leap { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let months = if leap {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 0usize;
    while days >= months[mo] {
        days -= months[mo];
        mo += 1;
    }
    let d = days + 1;
    format!("{y:04}-{:02}-{d:02}T{h:02}:{min:02}:{s:02}.000Z", mo + 1)
}

fn arn_to_sqs_url(arn: &str, default_region: &str, default_account: &str) -> String {
    // arn:aws:sqs:us-east-1:000000000000:my-queue
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    if parts.len() == 6 {
        let region = parts[3];
        let account = parts[4];
        let queue = parts[5];
        format!("http://sqs.{region}.localhost:4566/{account}/{queue}")
    } else {
        // ARN parse failed — try to use the last segment as a queue name.
        let queue = arn.rsplit(':').next().unwrap_or(arn);
        format!("http://sqs.{default_region}.localhost:4566/{default_account}/{queue}")
    }
}

/// Resolves a role's trust policy from the IAM service for STS AssumeRole.
struct IamTrustPolicyResolver(std::sync::Arc<awsim_iam::IamService>);

impl awsim_sts::TrustPolicyResolver for IamTrustPolicyResolver {
    fn resolve(&self, account_id: &str, role_arn: &str) -> Option<String> {
        self.0.lookup_role_trust_policy(account_id, role_arn)
    }
}

/// Bundle of handles returned by [`register_services`] for use by the router and OAuth layer.
type RegisteredServices = (
    Arc<awsim_apigateway::ApiGatewayService>,
    Arc<awsim_apigateway::ApiGatewayV1Service>,
    Arc<awsim_cognito::CognitoState>,
    Arc<awsim_iam::IamService>,
    awsim_core::AccountRegionStore<awsim_iam::state::IamState>,
    awsim_core::AccountRegionStore<awsim_s3::state::S3State>,
    awsim_core::AccountRegionStore<awsim_kms::state::KmsState>,
    awsim_core::AccountRegionStore<awsim_sqs::state::SqsState>,
    awsim_core::AccountRegionStore<awsim_secretsmanager::state::SecretsState>,
    awsim_core::AccountRegionStore<awsim_lambda::state::LambdaState>,
    awsim_core::AccountRegionStore<awsim_organizations::state::OrganizationsState>,
    Arc<awsim_ecr::EcrService>,
    Arc<awsim_s3::S3Service>,
    Arc<awsim_lambda::LambdaService>,
    Arc<awsim_sqs::SqsService>,
    Arc<awsim_cloudwatch_logs::CloudWatchLogsService>,
    awsim_core::AccountRegionStore<awsim_pipes::PipesState>,
    Arc<awsim_ec2::Ec2Service>,
    Arc<awsim_rds::RdsService>,
    Arc<awsim_mq::MqService>,
    Arc<awsim_memorydb::MemoryDbService>,
    Arc<awsim_dynamodb::DynamoDbService>,
    Arc<awsim_cloudwatch_metrics::CloudWatchMetricsService>,
    Arc<awsim_kinesis::KinesisService>,
    Arc<awsim_ses::SesService>,
    Arc<awsim_sts::StsSessionStore>,
);

/// Register all services and return handles needed by the router:
///   - the ApiGateway Arc (for proxy routing)
///   - an `Arc<CognitoState>` for the default account+region (for OAuth/OIDC)
// Build the runtime-config store. Path is `<data_dir>/runtime-config.json`
// when --data-dir is set; otherwise the store is in-memory only.
//
// CLI flags seed initial values. On first run with --data-dir, those
// values are persisted to disk; on subsequent runs, the file wins.
fn build_runtime_config_store(cli: &Cli) -> Result<Arc<runtime_config::RuntimeConfigStore>> {
    let seed = build_runtime_config_seed(cli)?;
    let path = cli
        .data_dir
        .as_deref()
        .map(|d| std::path::PathBuf::from(d).join(runtime_config::CONFIG_FILENAME));
    let store = runtime_config::RuntimeConfigStore::load_or_seed(seed, path)
        .context("loading runtime config")?;
    Ok(Arc::new(store))
}

// Translate startup CLI flags into a RuntimeConfig seed. The seed is
// only used as the initial state when no persisted file exists.
fn build_runtime_config_seed(cli: &Cli) -> Result<runtime_config::RuntimeConfig> {
    let bedrock_spec = if let Some(path) = cli.bedrock_config.as_deref() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str::<awsim_bedrock::BedrockSpec>(&raw)
            .with_context(|| format!("parsing bedrock config {}", path.display()))?
    } else if let Some(endpoint) = cli.bedrock_backend.as_deref() {
        let mut backends = std::collections::HashMap::new();
        backends.insert(
            "default".to_string(),
            awsim_bedrock::BackendSpec {
                endpoint: endpoint.to_string(),
                provider: None,
                credential: None,
                api_key: cli.bedrock_api_key.clone(),
                api_key_env: None,
            },
        );
        // --bedrock-model-map is a model-map-only TOML override: parse
        // its [invoke] / [embed] tables and overlay them.
        let (invoke, embed) = match cli.bedrock_model_map.as_deref() {
            Some(p) => {
                let raw = std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?;
                let parsed: ModelMapToml = toml::from_str(&raw)
                    .with_context(|| format!("parsing model map {}", p.display()))?;
                (parsed.invoke, parsed.embed)
            }
            None => (Default::default(), Default::default()),
        };
        awsim_bedrock::BedrockSpec {
            default_backend: Some("default".into()),
            credentials: Default::default(),
            backends,
            aliases: Default::default(),
            invoke,
            embed,
        }
    } else {
        awsim_bedrock::BedrockSpec::default()
    };
    let bedrock_enabled = !bedrock_spec.backends.is_empty();
    Ok(runtime_config::RuntimeConfig {
        bedrock: runtime_config::BedrockSection {
            enabled: bedrock_enabled,
            spec: bedrock_spec,
        },
        ses: runtime_config::SesSection {
            retention_hours: cli.ses_retention_hours,
        },
        iam: runtime_config::IamSection {
            // Seed from the existing AWSIM_IAM_ENFORCE env var so the
            // CLI / env-var path keeps working on first run.
            enforce: std::env::var("AWSIM_IAM_ENFORCE").ok().as_deref() == Some("true"),
        },
        logging: runtime_config::LoggingSection {
            level: cli.log_level.clone(),
        },
    })
}

#[derive(serde::Deserialize, Default)]
struct ModelMapToml {
    #[serde(default)]
    invoke: std::collections::HashMap<String, awsim_bedrock::ModelEntry>,
    #[serde(default)]
    embed: std::collections::HashMap<String, awsim_bedrock::ModelEntry>,
}

// Translate a runtime-config snapshot into a live BedrockBackends
// registry. `Ok(None)` means canned-response mode (proxy disabled or
// no backends declared). The health registry is attached so the
// alias resolver can skip Down targets and the runtime layer can
// drive error-fallback across all alias candidates.
fn build_bedrock_backend_from_config(
    cfg: &runtime_config::RuntimeConfig,
    health: &awsim_bedrock::HealthRegistry,
    metrics: &awsim_bedrock::MetricsRegistry,
    recent: &awsim_bedrock::RecentInvocations,
) -> Result<Option<awsim_bedrock::BedrockBackends>> {
    if !cfg.bedrock.enabled || cfg.bedrock.spec.backends.is_empty() {
        return Ok(None);
    }
    let backends =
        awsim_bedrock::build_from_spec(cfg.bedrock.spec.clone(), |v| std::env::var(v).ok())
            .context("building bedrock backends from runtime config")?
            .with_health(health.clone())
            .with_metrics(metrics.clone(), recent.clone());
    info!(
        backends = ?backends.backend_names(),
        default = ?backends.default_name(),
        "Bedrock proxy backend enabled"
    );
    Ok(Some(backends))
}

// Wires every service crate into the gateway at startup. The arg list
// grows by one each time a new cross-cutting subsystem is added
// (persistence dir, blob cap, WAL checkpoint, bedrock proxy); bundling
// them into a config struct adds indirection without removing any of
// the coupling, so the lint is suppressed deliberately here.
#[allow(clippy::too_many_arguments)]
fn register_services(
    state: &mut AppState,
    default_account_id: &str,
    default_region: &str,
    data_dir: Option<&str>,
    port: u16,
    max_blob_bytes: Option<u64>,
    ddb_wal_checkpoint_secs: Option<u64>,
    bedrock_swap: awsim_bedrock::BedrockBackendsSwap,
) -> RegisteredServices {
    use std::sync::Arc;

    let iam = Arc::new(awsim_iam::IamService::new());
    let iam_store = iam.store();
    let iam_service_clone = Arc::clone(&iam);
    state.register(iam, vec![]);

    // Shared STS session store: STS records every assumed-role
    // credential it issues (and Cognito Identity does the same below)
    // so the principal-lookup chain can resolve `ASIA…` keys back to
    // the assumed role on follow-up signed requests.
    let sts_sessions = Arc::new(awsim_sts::StsSessionStore::new());
    let sts = Arc::new(awsim_sts::StsService::with_session_store(Arc::clone(
        &sts_sessions,
    )));
    sts.set_trust_policy_resolver(Arc::new(IamTrustPolicyResolver(Arc::clone(
        &iam_service_clone,
    ))));
    state.register(sts, vec![]);

    let sns = Arc::new(awsim_sns::SnsService::new());
    state.register(sns, vec![]);

    let sqs = match data_dir {
        Some(dir) => {
            let svc = awsim_sqs::SqsService::with_data_dir(dir);
            match max_blob_bytes {
                Some(n) => svc.with_max_blob_bytes(n),
                None => svc,
            }
        }
        None => awsim_sqs::SqsService::new(),
    };
    let sqs_store = sqs.store();
    let sqs_arc = Arc::new(sqs);
    let sqs_clone = Arc::clone(&sqs_arc);
    state.register(sqs_arc, vec![]);

    let dynamodb = Arc::new(match data_dir {
        Some(dir) => awsim_dynamodb::DynamoDbService::with_data_dir(dir),
        None => awsim_dynamodb::DynamoDbService::new(),
    });
    // Background TTL sweeper — deletes items past their TTL once per
    // minute. Real DynamoDB allows up to ~48h slack; we're aggressive
    // since this is a dev tool and the sweep is cheap.
    dynamodb.spawn_ttl_sweeper(60);
    // Background WAL checkpointer — the inline PASSIVE autocheckpoint
    // starves under a sustained write firehose (bulk imports), so the
    // `-wal` file and its mapped index grow unbounded. A periodic
    // TRUNCATE checkpoint is the hard backstop. 0 opts out.
    match ddb_wal_checkpoint_secs {
        Some(0) => info!("DynamoDB WAL checkpointer disabled"),
        secs => {
            let interval = secs.unwrap_or(60);
            info!(
                interval_secs = interval,
                "DynamoDB WAL checkpointer enabled"
            );
            dynamodb.spawn_wal_checkpointer(interval);
        }
    }
    let dynamodb_clone = Arc::clone(&dynamodb);
    state.register(dynamodb, vec![]);

    let s3 = match data_dir {
        Some(dir) => {
            let svc = awsim_s3::S3Service::with_data_dir(dir);
            match max_blob_bytes {
                Some(n) => svc.with_max_blob_bytes(n),
                None => svc,
            }
        }
        None => awsim_s3::S3Service::new(),
    };
    let s3_store = s3.store();
    let s3_routes = {
        use awsim_core::ServiceHandler;
        s3.routes()
    };
    let s3_arc = Arc::new(s3);
    let s3_clone = Arc::clone(&s3_arc);
    state.register(s3_arc, s3_routes);

    let lambda = match data_dir {
        Some(dir) => {
            let svc = awsim_lambda::LambdaService::with_data_dir(dir);
            match max_blob_bytes {
                Some(n) => svc.with_max_blob_bytes(n),
                None => svc,
            }
        }
        None => awsim_lambda::LambdaService::new(),
    };
    let lambda_store = lambda.store();
    let lambda_routes = {
        use awsim_core::ServiceHandler;
        lambda.routes()
    };
    let lambda_arc = Arc::new(lambda);
    let lambda_clone = Arc::clone(&lambda_arc);
    state.register(lambda_arc, lambda_routes);

    let logs = match data_dir {
        Some(dir) => {
            let svc = awsim_cloudwatch_logs::CloudWatchLogsService::with_data_dir(dir);
            match max_blob_bytes {
                Some(n) => svc.with_max_blob_bytes(n),
                None => svc,
            }
        }
        None => awsim_cloudwatch_logs::CloudWatchLogsService::new(),
    };
    let logs_arc = Arc::new(logs);
    let logs_clone = Arc::clone(&logs_arc);
    state.register(logs_arc, vec![]);

    let eventbridge = Arc::new(awsim_eventbridge::EventBridgeService::new());
    state.register(eventbridge, vec![]);

    let kms = Arc::new(awsim_kms::KmsService::new());
    let kms_store = kms.store();
    state.register(kms, vec![]);

    let secretsmanager = Arc::new(awsim_secretsmanager::SecretsManagerService::new());
    let secrets_store = secretsmanager.store();
    state.register(secretsmanager, vec![]);

    let ssm = Arc::new(awsim_ssm::SsmService::new());
    state.register(ssm, vec![]);

    let stepfunctions = Arc::new(awsim_stepfunctions::StepFunctionsService::new());
    state.register(stepfunctions, vec![]);

    let kinesis = Arc::new(match data_dir {
        Some(dir) => awsim_kinesis::KinesisService::with_data_dir(dir),
        None => awsim_kinesis::KinesisService::new(),
    });
    let kinesis_clone = Arc::clone(&kinesis);
    state.register(kinesis, vec![]);

    let ses_service = Arc::new(match data_dir {
        Some(dir) => awsim_ses::SesService::with_data_dir(dir),
        None => awsim_ses::SesService::new(),
    });
    let ses_routes = {
        use awsim_core::ServiceHandler;
        ses_service.routes()
    };
    state.register(Arc::clone(&ses_service) as _, ses_routes);

    // Cognito — keep an Arc so we can share its state with the OAuth router.
    let cognito = Arc::new(awsim_cognito::CognitoService::new());
    let cognito_arc_state = cognito.state_for(default_account_id, default_region);
    state.register(cognito, vec![]);

    let cognito_identity = Arc::new(awsim_cognito::CognitoIdentityService::with_session_store(
        Arc::clone(&sts_sessions),
    ));
    state.register(cognito_identity, vec![]);

    let ecr = match data_dir {
        Some(dir) => {
            let svc = awsim_ecr::EcrService::with_data_dir(dir);
            match max_blob_bytes {
                Some(n) => svc.with_max_blob_bytes(n),
                None => svc,
            }
        }
        None => awsim_ecr::EcrService::new(),
    };
    let ecr = Arc::new(ecr.with_port(port));
    let ecr_clone = Arc::clone(&ecr);
    state.register(ecr, vec![]);

    let ecs = Arc::new(awsim_ecs::EcsService::new());
    state.register(ecs, vec![]);

    let ec2 = Arc::new(awsim_ec2::Ec2Service::new());
    let ec2_clone = Arc::clone(&ec2);
    state.register(ec2, vec![]);

    let rds = Arc::new(awsim_rds::RdsService::new());
    let rds_clone = Arc::clone(&rds);
    state.register(rds, vec![]);

    let appsync = awsim_appsync::AppSyncService::new();
    let appsync_routes = {
        use awsim_core::ServiceHandler;
        appsync.routes()
    };
    state.register(Arc::new(appsync), appsync_routes);

    let bedrock = awsim_bedrock::BedrockService::new();
    let bedrock_routes = {
        use awsim_core::ServiceHandler;
        bedrock.routes()
    };
    state.register(Arc::new(bedrock), bedrock_routes);

    let bedrock_runtime = awsim_bedrock::BedrockRuntimeService::with_swap(bedrock_swap);
    let bedrock_runtime_routes = {
        use awsim_core::ServiceHandler;
        bedrock_runtime.routes()
    };
    state.register(Arc::new(bedrock_runtime), bedrock_runtime_routes);

    let cloudformation = Arc::new(awsim_cloudformation::CloudFormationService::new());
    state.register(cloudformation, vec![]);

    let route53 = awsim_route53::Route53Service::new();
    let route53_routes = {
        use awsim_core::ServiceHandler;
        route53.routes()
    };
    state.register(Arc::new(route53), route53_routes);

    let cloudwatch_metrics = Arc::new(match data_dir {
        Some(dir) => awsim_cloudwatch_metrics::CloudWatchMetricsService::with_data_dir(dir),
        None => awsim_cloudwatch_metrics::CloudWatchMetricsService::new(),
    });
    let cloudwatch_metrics_clone = Arc::clone(&cloudwatch_metrics);
    state.register(cloudwatch_metrics, vec![]);

    let athena = Arc::new(awsim_athena::AthenaService::new());
    state.register(athena, vec![]);

    let glue = Arc::new(awsim_glue::GlueService::new());
    state.register(glue, vec![]);

    let elb = Arc::new(awsim_elb::ElbService::new());
    state.register(elb, vec![]);

    let cloudfront = awsim_cloudfront::CloudFrontService::new();
    let cloudfront_routes = {
        use awsim_core::ServiceHandler;
        cloudfront.routes()
    };
    state.register(Arc::new(cloudfront), cloudfront_routes);

    let acm = Arc::new(awsim_acm::AcmService::new());
    state.register(acm, vec![]);

    let waf = Arc::new(awsim_waf::WafService::new());
    state.register(waf, vec![]);

    let scheduler = awsim_scheduler::SchedulerService::new();
    let scheduler_routes = {
        use awsim_core::ServiceHandler;
        scheduler.routes()
    };
    state.register(Arc::new(scheduler), scheduler_routes);

    let comprehend = Arc::new(awsim_comprehend::ComprehendService::new());
    state.register(comprehend, vec![]);

    let kendra = Arc::new(awsim_kendra::KendraService::new());
    state.register(kendra, vec![]);

    let organizations = Arc::new(awsim_organizations::OrganizationsService::new());
    let organizations_store = organizations.store();
    state.register(organizations, vec![]);

    let cloudtrail = Arc::new(awsim_cloudtrail::CloudTrailService::new());
    state.register(cloudtrail, vec![]);

    let eks = awsim_eks::EksService::new();
    let eks_routes = {
        use awsim_core::ServiceHandler;
        eks.routes()
    };
    state.register(Arc::new(eks), eks_routes);

    let firehose = Arc::new(awsim_firehose::FirehoseService::new());
    state.register(firehose, vec![]);

    let batch = awsim_batch::BatchService::new();
    let batch_routes = {
        use awsim_core::ServiceHandler;
        batch.routes()
    };
    state.register(Arc::new(batch), batch_routes);

    let sso_admin = Arc::new(awsim_sso_admin::SsoAdminService::new());
    state.register(sso_admin, vec![]);

    let datasync = Arc::new(awsim_datasync::DataSyncService::new());
    state.register(datasync, vec![]);

    let polly = awsim_polly::PollyService::new();
    let polly_routes = {
        use awsim_core::ServiceHandler;
        polly.routes()
    };
    state.register(Arc::new(polly), polly_routes);

    let resourcegroupstagging =
        Arc::new(awsim_resourcegroupstagging::ResourceGroupsTaggingService::new());
    state.register(resourcegroupstagging, vec![]);

    let pipes = awsim_pipes::PipesService::new();
    let pipes_store = pipes.store();
    let pipes_routes = {
        use awsim_core::ServiceHandler;
        pipes.routes()
    };
    state.register(Arc::new(pipes), pipes_routes);

    let efs = awsim_efs::EfsService::new();
    let efs_routes = {
        use awsim_core::ServiceHandler;
        efs.routes()
    };
    state.register(Arc::new(efs), efs_routes);

    let backup = awsim_backup::BackupService::new();
    let backup_routes = {
        use awsim_core::ServiceHandler;
        backup.routes()
    };
    state.register(Arc::new(backup), backup_routes);

    let app_autoscaling = Arc::new(awsim_application_autoscaling::AppAutoScalingService::new());
    state.register(app_autoscaling, vec![]);

    let xray = awsim_xray::XrayService::new();
    let xray_routes = {
        use awsim_core::ServiceHandler;
        xray.routes()
    };
    state.register(Arc::new(xray), xray_routes);

    let servicediscovery = Arc::new(awsim_servicediscovery::ServiceDiscoveryService::new());
    state.register(servicediscovery, vec![]);

    // Control plane + AppConfigData data plane both sign as `appconfig`;
    // register them as one facade so neither clobbers the other.
    let appconfig = awsim_appconfig::AppConfig::new();
    let appconfig_routes = {
        use awsim_core::ServiceHandler;
        appconfig.routes()
    };
    state.register(Arc::new(appconfig), appconfig_routes);

    let glacier = awsim_glacier::GlacierService::new();
    let glacier_routes = {
        use awsim_core::ServiceHandler;
        glacier.routes()
    };
    state.register(Arc::new(glacier), glacier_routes);

    let mq = awsim_mq::MqService::new();
    let mq_routes = {
        use awsim_core::ServiceHandler;
        mq.routes()
    };
    let mq_arc = Arc::new(mq);
    let mq_clone = Arc::clone(&mq_arc);
    state.register(mq_arc, mq_routes);

    let memorydb = Arc::new(awsim_memorydb::MemoryDbService::new());
    let memorydb_clone = Arc::clone(&memorydb);
    state.register(memorydb, vec![]);

    let qldb = awsim_qldb::QldbService::new();
    let qldb_routes = {
        use awsim_core::ServiceHandler;
        qldb.routes()
    };
    state.register(Arc::new(qldb), qldb_routes);

    let transfer = Arc::new(awsim_transfer::TransferService::new());
    state.register(transfer, vec![]);

    let pinpoint = awsim_pinpoint::PinpointService::new();
    let pinpoint_routes = {
        use awsim_core::ServiceHandler;
        pinpoint.routes()
    };
    state.register(Arc::new(pinpoint), pinpoint_routes);

    let identitystore = Arc::new(awsim_identitystore::IdentityStoreService::new());
    state.register(identitystore, vec![]);

    // API Gateway — register both the v2 (HTTP APIs, signs as `execute-api`)
    // and v1 (REST APIs, signs as `apigateway`) handlers.
    let apigateway = Arc::new(awsim_apigateway::ApiGatewayService::new());
    let apigw_routes = {
        use awsim_core::ServiceHandler;
        apigateway.routes()
    };
    let apigw_clone = Arc::clone(&apigateway);
    state.register(apigateway, apigw_routes);

    let apigw_v1 = Arc::new(awsim_apigateway::ApiGatewayV1Service::new());
    let apigw_v1_routes = {
        use awsim_core::ServiceHandler;
        apigw_v1.routes()
    };
    let apigw_v1_clone = Arc::clone(&apigw_v1);
    state.register(apigw_v1, apigw_v1_routes);

    (
        apigw_clone,
        apigw_v1_clone,
        cognito_arc_state,
        iam_service_clone,
        iam_store,
        s3_store,
        kms_store,
        sqs_store,
        secrets_store,
        lambda_store,
        organizations_store,
        ecr_clone,
        s3_clone,
        lambda_clone,
        sqs_clone,
        logs_clone,
        pipes_store,
        ec2_clone,
        rds_clone,
        mq_clone,
        memorydb_clone,
        dynamodb_clone,
        cloudwatch_metrics_clone,
        kinesis_clone,
        ses_service,
        sts_sessions,
    )
}

fn run_gc(
    s3: &awsim_s3::S3Service,
    lambda: &awsim_lambda::LambdaService,
    sqs: &awsim_sqs::SqsService,
    ecr: &awsim_ecr::EcrService,
) {
    gc_one("s3", s3.body_store(), awsim_s3::S3Service::GROUPS, s3);
    gc_one(
        "lambda",
        lambda.body_store(),
        awsim_lambda::LambdaService::GROUPS,
        lambda,
    );
    gc_one("sqs", sqs.body_store(), awsim_sqs::SqsService::GROUPS, sqs);
    gc_one("ecr", ecr.body_store(), awsim_ecr::EcrService::GROUPS, ecr);
}

fn gc_one(
    service: &str,
    body_store: Option<&Arc<BodyStore>>,
    groups: &[&str],
    inventory: &dyn BlobInventory,
) {
    let Some(bs) = body_store else {
        return;
    };
    let known: std::collections::HashSet<(String, String, String)> =
        inventory.known_blobs().into_iter().collect();
    match bs.gc_orphaned(groups, &known) {
        Ok((deleted, freed_bytes)) => {
            if deleted > 0 {
                info!(
                    service,
                    deleted, freed_bytes, "BodyStore GC reclaimed orphaned blobs"
                );
            } else {
                info!(service, "BodyStore GC found no orphans");
            }
        }
        Err(e) => {
            warn!(service, error = %e, "BodyStore GC failed");
        }
    }
}
