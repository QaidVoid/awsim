use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};

use awsim_core::{
    AppState, BlobInventory, BodyStore, BodyStoreHandle, PersistenceManager, RequestContext,
};

mod admin;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();

    let mut state = AppState::new(cli.region.clone(), cli.account_id.clone());

    // Register all services; get back the ApiGateway Arc for proxy routing and
    // an Arc<CognitoState> for the default account+region so the OAuth router
    // can share user-pool state with the CognitoService.
    let (
        apigw_service,
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

    if let Some(authz) = Arc::get_mut(&mut state.authz) {
        authz.principal_lookup = Arc::new(awsim_iam::authz::IamPrincipalLookup::new(iam_store));
        authz.resource_policy_lookups.insert(
            "s3".to_string(),
            Arc::new(awsim_s3::S3ResourcePolicyLookup::new(s3_store)),
        );
        authz.resource_policy_lookups.insert(
            "kms".to_string(),
            Arc::new(awsim_kms::KmsResourcePolicyLookup::new(kms_store)),
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
            Arc::new(awsim_lambda::LambdaResourcePolicyLookup::new(lambda_store)),
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
            info!("Snapshots saved. Exiting.");
            std::process::exit(0);
        });

        // Spawn periodic auto-save every 30 seconds.
        let services_for_autosave = Arc::clone(&state.services);
        let pm_autosave = Arc::new(PersistenceManager::new(data_dir));
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(30);
            loop {
                tokio::time::sleep(interval).await;
                pm_autosave.save_all(&services_for_autosave);
            }
        });
    }

    let service_count = state.services.len();

    // Spawn background event router — handles cross-service fan-out.
    spawn_event_router(&state);

    // Spawn SQS->Lambda poller: periodically polls SQS queues for event source mappings.
    let poll_services = Arc::clone(&state.services);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            integrations::poll_sqs_event_sources(&poll_services).await;
        }
    });

    // Spawn Kinesis->Lambda poller: periodically polls Kinesis streams for event source mappings.
    let kinesis_poll_services = Arc::clone(&state.services);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            integrations::poll_kinesis_event_sources(&kinesis_poll_services).await;
        }
    });

    // Build the API Gateway proxy state using the concrete Arc returned from register_services.
    let lambda_arc = state.services.get("lambda").cloned();

    let proxy_state = proxy::ProxyState {
        apigw: apigw_service,
        lambda: lambda_arc,
        default_account_id: cli.account_id.clone(),
        default_region: cli.region.clone(),
    };

    // Build the proxy sub-router (finalized with its own state).
    let proxy_router: axum::Router<()> = axum::Router::new()
        .route(
            "/restapis/{api_id}/{stage}/{*path}",
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
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state);

    // Build the OpenSearch (Elasticsearch-compatible) sub-router.
    // Nest OpenSearch under /opensearch prefix so it doesn't conflict with AWS routes.
    let opensearch_nested: axum::Router<()> = axum::Router::new().nest(
        "/opensearch",
        awsim_opensearch::router(Arc::new(awsim_opensearch::state::OpenSearchState::default())),
    );

    let ecr_router = awsim_ecr::router(ecr_service);

    // Merge all routers and add shared middleware.
    let app = cognito_oauth_router
        .merge(main_router)
        .merge(proxy_router)
        .merge(opensearch_nested)
        .merge(ecr_router)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)) // 100 MB
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cli.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

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

    let dynamodb = Arc::new(awsim_dynamodb::DynamoDbService::new());
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
    state.register(ec2, vec![]);

    let rds = Arc::new(awsim_rds::RdsService::new());
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

    // API Gateway — registered last so we can return a clone of the Arc.
    let apigateway = Arc::new(awsim_apigateway::ApiGatewayService::new());
    let apigw_routes = {
        use awsim_core::ServiceHandler;
        apigateway.routes()
    };
    let apigw_clone = Arc::clone(&apigateway);
    state.register(apigateway, apigw_routes);

    (
        apigw_clone,
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
