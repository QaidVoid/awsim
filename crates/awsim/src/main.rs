use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};

use awsim_core::{AppState, PersistenceManager, RequestContext};

mod admin;
mod integrations;

#[derive(Parser)]
#[command(name = "awsim", about = "AWSim — fully offline, free AWS development environment")]
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();

    let mut state = AppState::new(cli.region.clone(), cli.account_id.clone());

    // Register all services
    register_services(&mut state);

    // Persistence: restore snapshots if --data-dir was provided.
    if let Some(ref data_dir) = cli.data_dir {
        let pm = PersistenceManager::new(data_dir);
        info!(data_dir = %data_dir, "Persistence enabled — restoring snapshots");
        pm.restore_all(&state.services);

        // Spawn graceful-shutdown handler that saves snapshots on SIGINT/SIGTERM.
        let services_for_shutdown = Arc::clone(&state.services);
        let pm_shutdown = Arc::new(PersistenceManager::new(data_dir));
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
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

    let app = axum::Router::new()
        .route("/_awsim/health", axum::routing::get(admin::health))
        .route("/_awsim/services", axum::routing::get(admin::list_services))
        .route("/_awsim/config", axum::routing::get(admin::config))
        .fallback(awsim_core::gateway::handle_request)
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cli.port));
    info!(
        port = cli.port,
        region = %cli.region,
        account_id = %cli.account_id,
        services = service_count,
        "AWSim started"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
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
                    let message_id = event.detail["message_id"].as_str().unwrap_or("").to_string();
                    let topic_arn = event.detail["topic_arn"].as_str().unwrap_or("").to_string();

                    match event.event_type.as_str() {
                        "sns:Publish" if protocol == "sqs" => {
                            // endpoint is a queue ARN: arn:aws:sqs:{region}:{account}:{queue-name}
                            // Derive the queue URL so SQS SendMessage can find the queue.
                            let queue_url = arn_to_sqs_url(&endpoint, &default_region, &default_account_id);

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
                        _ => {
                            // Unknown or unhandled event type — ignore.
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Event bus receiver lagged; some events were dropped");
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
        format!(
            "http://sqs.{default_region}.localhost:4566/{default_account}/{queue}"
        )
    }
}

fn register_services(state: &mut AppState) {
    use std::sync::Arc;

    let iam = Arc::new(awsim_iam::IamService::new());
    state.register(iam, vec![]);

    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    let sns = Arc::new(awsim_sns::SnsService::new());
    state.register(sns, vec![]);

    let sqs = Arc::new(awsim_sqs::SqsService::new());
    state.register(sqs, vec![]);

    let dynamodb = Arc::new(awsim_dynamodb::DynamoDbService::new());
    state.register(dynamodb, vec![]);

    let s3 = awsim_s3::S3Service::new();
    let s3_routes = {
        use awsim_core::ServiceHandler;
        s3.routes()
    };
    state.register(Arc::new(s3), s3_routes);

    let lambda = awsim_lambda::LambdaService::new();
    let lambda_routes = {
        use awsim_core::ServiceHandler;
        lambda.routes()
    };
    state.register(Arc::new(lambda), lambda_routes);

    let logs = Arc::new(awsim_cloudwatch_logs::CloudWatchLogsService::new());
    state.register(logs, vec![]);

    let eventbridge = Arc::new(awsim_eventbridge::EventBridgeService::new());
    state.register(eventbridge, vec![]);

    let kms = Arc::new(awsim_kms::KmsService::new());
    state.register(kms, vec![]);

    let secretsmanager = Arc::new(awsim_secretsmanager::SecretsManagerService::new());
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

    let cognito = Arc::new(awsim_cognito::CognitoService::new());
    state.register(cognito, vec![]);

    let ecr = Arc::new(awsim_ecr::EcrService::new());
    state.register(ecr, vec![]);

    let ecs = Arc::new(awsim_ecs::EcsService::new());
    state.register(ecs, vec![]);

    let ec2 = Arc::new(awsim_ec2::Ec2Service::new());
    state.register(ec2, vec![]);

    let cloudformation = Arc::new(awsim_cloudformation::CloudFormationService::new());
    state.register(cloudformation, vec![]);
}
