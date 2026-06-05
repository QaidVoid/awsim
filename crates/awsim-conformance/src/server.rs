use std::sync::Arc;

use awsim_core::AppState;

/// Start an in-process AWSim server on a random available port.
/// Returns the base endpoint URL (e.g. "http://127.0.0.1:14566").
pub async fn start() -> String {
    let region = "us-east-1".to_string();
    let account_id = "000000000000".to_string();

    let mut state = AppState::new(region.clone(), account_id.clone());

    let (_apigw_service, cognito_state) = register_services(&mut state, &account_id, &region);

    // Build Cognito OAuth router (needed to avoid panics in the main router).
    let cognito_oauth_state = Arc::new(awsim_cognito::CognitoOAuthState {
        cognito: cognito_state,
        default_account_id: account_id.clone(),
        default_region: region.clone(),
        auth_codes: Arc::new(dashmap::DashMap::new()),
        revoked_refresh_tokens: Arc::new(dashmap::DashMap::new()),
        federation: awsim_cognito::federation::FederationState::new(),
        port: 0,
    });
    let cognito_oauth_router = awsim_cognito::oauth::router(cognito_oauth_state);

    // Main router: the gateway fallback handles all AWS API calls.
    let main_router: axum::Router<()> = axum::Router::new()
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state);

    let app = cognito_oauth_router
        .merge(main_router)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive());

    // Bind on a random port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind TCP listener");
    let addr = listener.local_addr().expect("Failed to get local addr");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Server error");
    });

    format!("http://127.0.0.1:{}", addr.port())
}

/// Start a standalone OpenSearch server on a random port, returning its base
/// URL. The OpenSearch REST data plane runs on its own dedicated endpoint in
/// production, so it gets its own listener here rather than being merged into
/// the gateway (where `PUT /{index}` would collide with S3 path-style buckets).
pub async fn start_opensearch() -> String {
    let state = std::sync::Arc::new(
        awsim_opensearch::state::OpenSearchState::ephemeral().expect("opensearch state"),
    );
    let app = awsim_opensearch::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind TCP listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Server error");
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Register all services — mirrors the logic in the awsim binary.
fn register_services(
    state: &mut AppState,
    default_account_id: &str,
    default_region: &str,
) -> (
    Arc<awsim_apigateway::ApiGatewayService>,
    Arc<awsim_cognito::CognitoState>,
) {
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
    let cognito_arc_state = cognito.state_for(default_account_id, default_region);
    state.register(cognito, vec![]);

    let cognito_identity = Arc::new(awsim_cognito::CognitoIdentityService::new());
    state.register(cognito_identity, vec![]);

    let ecr = Arc::new(awsim_ecr::EcrService::new());
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

    let datasync = Arc::new(awsim_datasync::DataSyncService::new());
    state.register(datasync, vec![]);

    let polly = awsim_polly::PollyService::new();
    let polly_routes = {
        use awsim_core::ServiceHandler;
        polly.routes()
    };
    state.register(Arc::new(polly), polly_routes);

    let sso_admin = Arc::new(awsim_sso_admin::SsoAdminService::new());
    state.register(sso_admin, vec![]);

    let apigateway = Arc::new(awsim_apigateway::ApiGatewayService::new());
    let apigw_routes = {
        use awsim_core::ServiceHandler;
        apigateway.routes()
    };
    let apigw_clone = Arc::clone(&apigateway);
    state.register(apigateway, apigw_routes);

    (apigw_clone, cognito_arc_state)
}
