use anyhow::Result;
use clap::Parser;
use tracing::info;

use awsim_core::AppState;

mod admin;

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

    let service_count = state.services.len();

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
