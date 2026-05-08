use std::sync::Arc;

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use awsim_core::{AppState, AuthzEngine, ScpLookup, ServiceHandler};
use awsim_iam_policy::PolicyDocument;

pub struct ServerHandle {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    authz: Arc<AuthzEngine>,
}

impl ServerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }

    /// Live toggle: enable IAM enforcement without restarting the
    /// server, mirroring the runtime-config flip the UI performs.
    pub fn set_enforcement(&self, enabled: bool) {
        self.authz.set_enabled(enabled);
    }
}

pub struct StaticScpLookup {
    pub policies: Vec<PolicyDocument>,
}

impl ScpLookup for StaticScpLookup {
    fn lookup(&self, _principal_arn: &str) -> Vec<PolicyDocument> {
        self.policies.clone()
    }
}

pub async fn start_server(enforce: bool, iam: Arc<awsim_iam::IamService>) -> (ServerHandle, u16) {
    start_server_with_scp(enforce, iam, None).await
}

pub async fn start_server_with_scp(
    enforce: bool,
    iam: Arc<awsim_iam::IamService>,
    scp: Option<Arc<dyn ScpLookup>>,
) -> (ServerHandle, u16) {
    unsafe {
        if enforce {
            std::env::set_var("AWSIM_IAM_ENFORCE", "true");
        } else {
            std::env::set_var("AWSIM_IAM_ENFORCE", "false");
        }
    }

    let mut state = AppState::new("us-east-1".into(), "000000000000".into());
    let iam_store = iam.store();
    state.register(iam.clone(), vec![]);

    // Shared STS session store: STS records issued temp creds here so
    // the principal-lookup chain can resolve `ASIA…` keys back to the
    // assumed role.
    let sts_sessions = Arc::new(awsim_sts::StsSessionStore::new());
    let sts = Arc::new(awsim_sts::StsService::with_session_store(Arc::clone(
        &sts_sessions,
    )));
    state.register(sts, vec![]);

    let s3 = awsim_s3::S3Service::new();
    let s3_store = s3.store();
    let s3_routes = s3.routes();
    state.register(Arc::new(s3), s3_routes);

    let dynamodb = Arc::new(awsim_dynamodb::DynamoDbService::new());
    state.register(dynamodb, vec![]);

    let lambda = awsim_lambda::LambdaService::new();
    let lambda_store = lambda.store();
    let lambda_routes = lambda.routes();
    state.register(Arc::new(lambda), lambda_routes);

    let sqs = awsim_sqs::SqsService::new();
    let sqs_store = sqs.store();
    state.register(Arc::new(sqs), vec![]);

    let sns = Arc::new(awsim_sns::SnsService::new());
    state.register(sns, vec![]);

    let secrets = awsim_secretsmanager::SecretsManagerService::new();
    let secrets_store = secrets.store();
    state.register(Arc::new(secrets), vec![]);

    let kms = awsim_kms::KmsService::new();
    let kms_store = kms.store();
    state.register(Arc::new(kms), vec![]);

    let mut authz = AuthzEngine::new(enforce);
    let iam_lookup: Arc<dyn awsim_core::PrincipalLookup> =
        Arc::new(awsim_iam::authz::IamPrincipalLookup::new(iam_store));
    authz.principal_lookup = Arc::new(awsim_sts::StsAwarePrincipalLookup::new(
        Arc::clone(&sts_sessions),
        iam_lookup,
    ));
    authz.resource_policy_lookups.insert(
        "s3".to_string(),
        Arc::new(awsim_s3::S3ResourcePolicyLookup::new(s3_store))
            as Arc<dyn awsim_core::ResourcePolicyLookup>,
    );
    authz.resource_policy_lookups.insert(
        "lambda".to_string(),
        Arc::new(awsim_lambda::LambdaResourcePolicyLookup::new(lambda_store))
            as Arc<dyn awsim_core::ResourcePolicyLookup>,
    );
    authz.resource_policy_lookups.insert(
        "sqs".to_string(),
        Arc::new(awsim_sqs::SqsResourcePolicyLookup::new(sqs_store))
            as Arc<dyn awsim_core::ResourcePolicyLookup>,
    );
    authz.resource_policy_lookups.insert(
        "secretsmanager".to_string(),
        Arc::new(awsim_secretsmanager::SecretsManagerResourcePolicyLookup::new(secrets_store))
            as Arc<dyn awsim_core::ResourcePolicyLookup>,
    );
    authz.resource_policy_lookups.insert(
        "kms".to_string(),
        Arc::new(awsim_kms::KmsResourcePolicyLookup::new(kms_store.clone()))
            as Arc<dyn awsim_core::ResourcePolicyLookup>,
    );
    authz.grant_lookups.insert(
        "kms".to_string(),
        Arc::new(awsim_kms::KmsGrantLookup::new(kms_store)) as Arc<dyn awsim_core::GrantLookup>,
    );
    authz.scp_lookup = scp;
    let authz_arc = Arc::new(authz);
    state.authz = Arc::clone(&authz_arc);

    let app: axum::Router<()> = axum::Router::new()
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });

    (
        ServerHandle {
            shutdown: tx,
            task,
            authz: authz_arc,
        },
        port,
    )
}

pub async fn start_server_enforced(iam: Arc<awsim_iam::IamService>) -> (ServerHandle, u16) {
    start_server(true, iam).await
}

pub fn with_scp(scp_doc: &str) -> Arc<dyn ScpLookup> {
    let policy = awsim_iam_policy::parse(scp_doc).expect("SCP parses");
    Arc::new(StaticScpLookup {
        policies: vec![policy],
    })
}

pub async fn start_server_enforced_with_scp(
    iam: Arc<awsim_iam::IamService>,
    scp: Arc<dyn ScpLookup>,
) -> (ServerHandle, u16) {
    start_server_with_scp(true, iam, Some(scp)).await
}

pub async fn start_server_unenforced(iam: Arc<awsim_iam::IamService>) -> (ServerHandle, u16) {
    start_server(false, iam).await
}

pub fn make_sdk_config(port: u16, access_key: &str, secret: &str) -> SdkConfig {
    aws_config::SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .region(Region::new("us-east-1"))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(Credentials::new(
                access_key, secret, None, None, "test",
            )),
        )
        .build()
}

pub fn s3_client(cfg: &SdkConfig) -> aws_sdk_s3::Client {
    let s3_cfg = aws_sdk_s3::config::Builder::from(cfg)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_cfg)
}

pub fn iam_client(cfg: &SdkConfig) -> aws_sdk_iam::Client {
    aws_sdk_iam::Client::new(cfg)
}

pub async fn bootstrap_user(
    iam: &aws_sdk_iam::Client,
    user: &str,
    policies: &[(String, String)],
) -> (String, String) {
    iam.create_user()
        .user_name(user)
        .send()
        .await
        .expect("create_user");
    let ak = iam
        .create_access_key()
        .user_name(user)
        .send()
        .await
        .expect("create_access_key");
    let k = ak.access_key.expect("access_key returned");

    for (name, doc) in policies {
        let created = iam
            .create_policy()
            .policy_name(name)
            .policy_document(doc)
            .send()
            .await
            .expect("create_policy");
        let arn = created.policy.and_then(|p| p.arn).expect("policy arn");
        iam.attach_user_policy()
            .user_name(user)
            .policy_arn(arn)
            .send()
            .await
            .expect("attach_user_policy");
    }

    (k.access_key_id, k.secret_access_key)
}

pub fn sdk_err_is_access_denied<E>(
    err: &SdkError<E, aws_smithy_runtime_api::http::Response>,
) -> bool
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    if let SdkError::ServiceError(ctx) = err
        && matches!(
            ctx.err().code(),
            Some("AccessDenied") | Some("AccessDeniedException")
        )
    {
        return true;
    }
    err.raw_response()
        .map(|r| r.status().as_u16() == 403)
        .unwrap_or(false)
}

pub const ALLOW_GETOBJECT: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": "s3:GetObject",
    "Resource": "arn:aws:s3:::test-bucket/*"
  }]
}"#;

pub const ALLOW_ALL_S3: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": "s3:*",
    "Resource": "*"
  }]
}"#;

pub const DENY_PUTOBJECT: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Deny",
    "Action": "s3:PutObject",
    "Resource": "arn:aws:s3:::test-bucket/*"
  }]
}"#;

pub const ALLOW_DDB_TABLE: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["dynamodb:CreateTable","dynamodb:DescribeTable","dynamodb:PutItem","dynamodb:GetItem","dynamodb:Query","dynamodb:Scan","dynamodb:DeleteTable"],
    "Resource": "arn:aws:dynamodb:us-east-1:000000000000:table/widgets"
  }]
}"#;

pub const ALLOW_LAMBDA_INVOKE: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": "lambda:InvokeFunction",
    "Resource": "arn:aws:lambda:us-east-1:000000000000:function:hello"
  }]
}"#;

pub const ALLOW_SQS_SEND: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["sqs:CreateQueue","sqs:GetQueueUrl","sqs:GetQueueAttributes","sqs:SendMessage","sqs:ReceiveMessage","sqs:DeleteMessage"],
    "Resource": "arn:aws:sqs:us-east-1:000000000000:work"
  }]
}"#;

pub const ALLOW_SNS_PUBLISH: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["sns:CreateTopic","sns:Publish","sns:GetTopicAttributes"],
    "Resource": "arn:aws:sns:us-east-1:000000000000:alerts"
  }]
}"#;

pub const ALLOW_SECRET_READ: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["secretsmanager:CreateSecret","secretsmanager:GetSecretValue","secretsmanager:DescribeSecret"],
    "Resource": "*"
  }]
}"#;

pub const ALLOW_IAM_RO: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["iam:GetUser","iam:ListUsers","iam:ListPolicies"],
    "Resource": "*"
  }]
}"#;

pub fn dynamodb_client(cfg: &SdkConfig) -> aws_sdk_dynamodb::Client {
    aws_sdk_dynamodb::Client::new(cfg)
}

pub fn lambda_client(cfg: &SdkConfig) -> aws_sdk_lambda::Client {
    aws_sdk_lambda::Client::new(cfg)
}

pub fn sqs_client(cfg: &SdkConfig) -> aws_sdk_sqs::Client {
    aws_sdk_sqs::Client::new(cfg)
}

pub fn sns_client(cfg: &SdkConfig) -> aws_sdk_sns::Client {
    aws_sdk_sns::Client::new(cfg)
}

pub fn secretsmanager_client(cfg: &SdkConfig) -> aws_sdk_secretsmanager::Client {
    aws_sdk_secretsmanager::Client::new(cfg)
}

pub fn kms_client(cfg: &SdkConfig) -> aws_sdk_kms::Client {
    aws_sdk_kms::Client::new(cfg)
}
