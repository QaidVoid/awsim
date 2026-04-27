use std::sync::Arc;

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use awsim_core::{AppState, AuthzEngine, ScpLookup, ServiceHandler};
use awsim_iam_policy::PolicyDocument;

pub struct ServerHandle {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
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

    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    let s3 = awsim_s3::S3Service::new();
    let s3_store = s3.store();
    let s3_routes = s3.routes();
    state.register(Arc::new(s3), s3_routes);

    let authz = AuthzEngine {
        enabled: enforce,
        principal_lookup: Arc::new(awsim_iam::authz::IamPrincipalLookup::new(iam_store)),
        resource_policy_lookups: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "s3".to_string(),
                Arc::new(awsim_s3::S3ResourcePolicyLookup::new(s3_store))
                    as Arc<dyn awsim_core::ResourcePolicyLookup>,
            );
            m
        },
        scp_lookup: scp,
    };
    state.authz = Arc::new(authz);

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

    (ServerHandle { shutdown: tx, task }, port)
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
