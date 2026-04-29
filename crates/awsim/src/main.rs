use anyhow::Result;
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

use awsim_core::{
    AppState, BlobInventory, BodyStore, BodyStoreHandle, PersistenceManager, RequestContext,
};

mod admin;
mod bill_cli;
mod chaos_cli;
mod integrations;
mod proxy;

#[derive(Parser)]
#[command(
    name = "awsim",
    about = "AWSim — fully offline, free AWS development environment"
)]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "4566", env = "AWSIM_PORT")]
    port: u16,

    /// Default AWS region
    #[arg(short, long, default_value = "us-east-1", env = "AWSIM_REGION")]
    region: String,

    /// Default AWS account ID
    #[arg(long, default_value = "000000000000", env = "AWSIM_ACCOUNT_ID")]
    account_id: String,

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

    /// Maximum concurrent in-flight HTTP requests. Requests above this cap
    /// are immediately rejected with 503 Service Unavailable instead of
    /// queuing — so a misbehaving client (e.g. one leaking connections
    /// during a bulk import) can't accumulate work that eventually
    /// exhausts file descriptors or memory.
    #[arg(long, env = "AWSIM_MAX_CONCURRENT_REQUESTS", default_value_t = 5_000)]
    max_concurrent_requests: usize,

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

#[tokio::main]
async fn main() -> Result<()> {
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
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();

    raise_nofile_limit();

    let mut state = AppState::new(cli.region.clone(), cli.account_id.clone());

    // Register all services; get back the ApiGateway Arc for proxy routing and
    // an Arc<CognitoState> for the default account+region so the OAuth router
    // can share user-pool state with the CognitoService.
    let (
        apigw_service,
        apigw_v1_service,
        cognito_state,
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
    ) = register_services(
        &mut state,
        &cli.account_id,
        &cli.region,
        cli.data_dir.as_deref(),
        cli.port,
        cli.max_blob_bytes,
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
    if let Some(bs) = logs_service.body_store() {
        body_stores.push(BodyStoreHandle {
            service_name: "cloudwatch-logs".to_string(),
            groups: awsim_cloudwatch_logs::CloudWatchLogsService::GROUPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            body_store: Arc::clone(bs),
        });
    }
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
        authz.principal_lookup = Arc::new(awsim_iam::authz::IamPrincipalLookup::new(iam_store));
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
            Arc::new(awsim_sqs::SqsResourcePolicyLookup::new(sqs_store)),
        );
        authz.resource_policy_lookups.insert(
            "secretsmanager".to_string(),
            Arc::new(awsim_secretsmanager::SecretsManagerResourcePolicyLookup::new(secrets_store)),
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

        if !cli.no_gc {
            run_gc(
                s3_service.as_ref(),
                lambda_service.as_ref(),
                sqs_service.as_ref(),
                ecr_service.as_ref(),
                logs_service.as_ref(),
            );
        }

        if let Some(secs) = cli.gc_interval_secs {
            let s3_gc = Arc::clone(&s3_service);
            let lambda_gc = Arc::clone(&lambda_service);
            let sqs_gc = Arc::clone(&sqs_service);
            let ecr_gc = Arc::clone(&ecr_service);
            let logs_gc = Arc::clone(&logs_service);
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
                        logs_gc.as_ref(),
                    );
                }
            });
        }

        // Spawn graceful-shutdown handler that saves snapshots on SIGINT/SIGTERM.
        let services_for_shutdown = Arc::clone(&state.services);
        let pm_shutdown = Arc::new(PersistenceManager::new(data_dir));
        let billing_for_shutdown = Arc::clone(&billing_meter);
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

            info!("Shutdown signal received — saving snapshots...");
            pm_shutdown.save_all(&services_for_shutdown);
            if let Some(bytes) = billing_for_shutdown.store.snapshot_to_bytes()
                && let Err(e) = pm_shutdown.save_snapshot("billing", &bytes)
            {
                warn!(error = %e, "Failed to save billing snapshot on shutdown");
            }
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
        cognito: cognito_state,
        default_account_id: cli.account_id.clone(),
        default_region: cli.region.clone(),
        auth_codes: Arc::new(dashmap::DashMap::new()),
        revoked_refresh_tokens: Arc::new(dashmap::DashMap::new()),
        port: cli.port,
    });
    let cognito_oauth_router = awsim_cognito::oauth::router(cognito_oauth_state);

    // Build the main router (finalized with AppState).
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
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state.clone());

    // Build the OpenSearch (Elasticsearch-compatible) sub-router.
    // Nest OpenSearch under /opensearch prefix so it doesn't conflict with AWS routes.
    let opensearch_nested: axum::Router<()> = axum::Router::new().nest(
        "/opensearch",
        awsim_opensearch::router(Arc::new(awsim_opensearch::state::OpenSearchState::default())),
    );

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

    // Merge all routers and add shared middleware.
    let app = cognito_oauth_router
        .merge(main_router)
        .merge(proxy_router)
        .merge(opensearch_nested)
        .merge(ecr_router)
        .merge(billing_router)
        .merge(chaos_router)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)) // 100 MB
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

    // Bind to the IPv6 unspecified address (`[::]`) so we accept both
    // IPv6 and IPv4-mapped connections on a single socket. Node 20+
    // resolves `localhost` to `::1` first, so an IPv4-only bind on
    // `0.0.0.0` makes the SDK fail with ECONNREFUSED before any request
    // ever reaches us. If the OS has `net.ipv6.bindv6only=1` (uncommon
    // on dev machines) or IPv6 is disabled, fall back to plain v4.
    let addr_v6 = std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, cli.port));
    let listener = match tokio::net::TcpListener::bind(addr_v6).await {
        Ok(l) => l,
        Err(e) => {
            warn!(error = %e, "Could not bind on [::] — falling back to 0.0.0.0");
            let addr_v4 = std::net::SocketAddr::from(([0, 0, 0, 0], cli.port));
            tokio::net::TcpListener::bind(addr_v4).await?
        }
    };

    // Startup banner
    println!();
    println!("  AWSim v{}", env!("CARGO_PKG_VERSION"));
    println!("  Fully Offline AWS Dev Environment");
    println!();
    println!("  Endpoint:  http://localhost:{}", cli.port);
    println!("  Region:    {}", cli.region);
    println!("  Account:   {}", cli.account_id);
    println!("  Services:  {} registered", service_count);
    if let Some(ref data_dir) = cli.data_dir {
        println!("  Persist:   {}", data_dir);
    }
    println!();

    info!(
        port = cli.port,
        region = %cli.region,
        account_id = %cli.account_id,
        services = service_count,
        "AWSim started"
    );

    axum::serve(listener, app).await?;

    Ok(())
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
                                    service: "sqs".to_string(),
                                    access_key: None,
                                    request_id: uuid::Uuid::new_v4().to_string(),
                                    method: "POST".to_string(),
                                    uri: "/".to_string(),
                                    event_bus: None,
                                };

                                // Wrap the SNS message in the SNS notification envelope that
                                // real AWS delivers to SQS subscribers.
                                let body = serde_json::json!({
                                    "Type": "Notification",
                                    "MessageId": message_id,
                                    "TopicArn": topic_arn,
                                    "Message": message,
                                })
                                .to_string();

                                let input = serde_json::json!({
                                    "QueueUrl": queue_url,
                                    "MessageBody": body,
                                });

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

/// Bundle of handles returned by [`register_services`] for use by the router and OAuth layer.
type RegisteredServices = (
    Arc<awsim_apigateway::ApiGatewayService>,
    Arc<awsim_apigateway::ApiGatewayV1Service>,
    Arc<awsim_cognito::CognitoState>,
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
);

/// Register all services and return handles needed by the router:
///   - the ApiGateway Arc (for proxy routing)
///   - an `Arc<CognitoState>` for the default account+region (for OAuth/OIDC)
fn register_services(
    state: &mut AppState,
    default_account_id: &str,
    default_region: &str,
    data_dir: Option<&str>,
    port: u16,
    max_blob_bytes: Option<u64>,
) -> RegisteredServices {
    use std::sync::Arc;

    let iam = Arc::new(awsim_iam::IamService::new());
    let iam_store = iam.store();
    state.register(iam, vec![]);

    let sts = Arc::new(awsim_sts::StsService::new());
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

    let kinesis = Arc::new(awsim_kinesis::KinesisService::new());
    state.register(kinesis, vec![]);

    let ses = awsim_ses::SesService::new();
    let ses_routes = {
        use awsim_core::ServiceHandler;
        ses.routes()
    };
    state.register(Arc::new(ses), ses_routes);

    // Cognito — keep an Arc so we can share its state with the OAuth router.
    let cognito = Arc::new(awsim_cognito::CognitoService::new());
    let cognito_arc_state = cognito.state_for(default_account_id, default_region);
    state.register(cognito, vec![]);

    let cognito_identity = Arc::new(awsim_cognito::CognitoIdentityService::new());
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

    let bedrock_runtime = awsim_bedrock::BedrockRuntimeService::new();
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

    let cloudwatch_metrics = Arc::new(awsim_cloudwatch_metrics::CloudWatchMetricsService::new());
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

    let appconfig = awsim_appconfig::AppConfigService::new();
    let appconfig_store = appconfig.store();
    let appconfig_routes = {
        use awsim_core::ServiceHandler;
        appconfig.routes()
    };
    state.register(Arc::new(appconfig), appconfig_routes);

    let appconfigdata = awsim_appconfig::AppConfigDataService::new(appconfig_store);
    let appconfigdata_routes = {
        use awsim_core::ServiceHandler;
        appconfigdata.routes()
    };
    state.register(Arc::new(appconfigdata), appconfigdata_routes);

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
    )
}

fn run_gc(
    s3: &awsim_s3::S3Service,
    lambda: &awsim_lambda::LambdaService,
    sqs: &awsim_sqs::SqsService,
    ecr: &awsim_ecr::EcrService,
    logs: &awsim_cloudwatch_logs::CloudWatchLogsService,
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
    gc_one(
        "cloudwatch-logs",
        logs.body_store(),
        awsim_cloudwatch_logs::CloudWatchLogsService::GROUPS,
        logs,
    );
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
