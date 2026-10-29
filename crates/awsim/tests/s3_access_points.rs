//! S3 Control Access Points through the official SDKs.
//!
//! With a custom endpoint the SDKs put the account id in front of its host
//! for S3 Control (`000000000000.localhost`) and address an Access Point
//! ARN virtual-hosted (`owner-1-000000000000.localhost`). The endpoint here
//! is `localhost` so both resolve to loopback, as `*.localhost` does under
//! systemd-resolved and on macOS.

use std::sync::Arc;

use aws_credential_types::Credentials;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3control::types::{PublicAccessBlockConfiguration, VpcConfiguration};
use awsim_core::{AppState, ServiceHandler};

const ACCOUNT: &str = "000000000000";
const ARN: &str = "arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1";

async fn start_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut state = AppState::new("us-east-1".into(), ACCOUNT.into());
    let s3 = awsim_s3::S3Service::new();
    let routes = s3.routes();
    state.register(Arc::new(s3), routes);
    let app = axum::Router::new()
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    format!("http://localhost:{port}")
}

async fn config(endpoint: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await
}

async fn clients(endpoint: &str) -> (aws_sdk_s3control::Client, aws_sdk_s3::Client) {
    let config = config(endpoint).await;
    (
        aws_sdk_s3control::Client::new(&config),
        aws_sdk_s3::Client::new(&config),
    )
}

async fn with_bucket(endpoint: &str) -> (aws_sdk_s3control::Client, aws_sdk_s3::Client) {
    let (control, s3) = clients(endpoint).await;
    let path_style = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&config(endpoint).await)
            .force_path_style(true)
            .build(),
    );
    path_style
        .create_bucket()
        .bucket("data")
        .send()
        .await
        .expect("create_bucket");
    (control, s3)
}

fn code<E, R>(err: &aws_sdk_s3control::error::SdkError<E, R>) -> String
where
    E: aws_sdk_s3control::error::ProvideErrorMetadata,
{
    err.as_service_error()
        .and_then(|e| e.code())
        .unwrap_or("not a service error")
        .to_string()
}

#[tokio::test]
async fn test_create_get_list_and_delete_access_point() {
    let endpoint = start_server().await;
    let (control, _) = with_bucket(&endpoint).await;

    let created = control
        .create_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .bucket("data")
        .vpc_configuration(VpcConfiguration::builder().vpc_id("vpc-1").build().unwrap())
        .public_access_block_configuration(
            PublicAccessBlockConfiguration::builder()
                .block_public_acls(true)
                .ignore_public_acls(true)
                .block_public_policy(true)
                .restrict_public_buckets(true)
                .build(),
        )
        .send()
        .await
        .expect("create_access_point");
    assert_eq!(created.access_point_arn(), Some(ARN));
    let alias = created.alias().unwrap().to_string();
    assert!(alias.starts_with("owner-1-") && alias.ends_with("-s3alias"));

    let got = control
        .get_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .expect("get_access_point");
    assert_eq!(got.name(), Some("owner-1"));
    assert_eq!(got.bucket(), Some("data"));
    assert_eq!(got.network_origin().map(|o| o.as_str()), Some("VPC"));
    assert_eq!(got.vpc_configuration().map(|v| v.vpc_id()), Some("vpc-1"));
    assert_eq!(got.access_point_arn(), Some(ARN));
    assert_eq!(got.alias(), Some(alias.as_str()));
    assert_eq!(got.bucket_account_id(), Some(ACCOUNT));
    assert!(got.creation_date().is_some());
    let block = got.public_access_block_configuration().unwrap();
    assert_eq!(block.restrict_public_buckets(), Some(true));
    assert!(got.endpoints().is_some_and(|e| e.contains_key("ipv4")));

    let listed = control
        .list_access_points()
        .account_id(ACCOUNT)
        .bucket("data")
        .send()
        .await
        .expect("list_access_points");
    let names: Vec<&str> = listed
        .access_point_list()
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(names, ["owner-1"]);
    assert_eq!(listed.access_point_list()[0].alias(), Some(alias.as_str()));

    control
        .delete_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .expect("delete_access_point");
    let err = control
        .get_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .unwrap_err();
    assert_eq!(code(&err), "NoSuchAccessPoint");
    let err = control
        .delete_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .unwrap_err();
    assert_eq!(code(&err), "NoSuchAccessPoint");
}

#[tokio::test]
async fn test_create_access_point_errors() {
    let endpoint = start_server().await;
    let (control, _) = with_bucket(&endpoint).await;
    let create = |name: &str, bucket: &str| {
        control
            .create_access_point()
            .account_id(ACCOUNT)
            .name(name)
            .bucket(bucket)
            .send()
    };

    create("owner-1", "data").await.expect("first create");
    let err = create("owner-1", "data").await.unwrap_err();
    assert_eq!(code(&err), "AccessPointAlreadyOwnedByYou");

    let err = create("Bad_Name", "data").await.unwrap_err();
    assert_eq!(code(&err), "InvalidAccessPoint");

    let err = create("owner-2", "absent").await.unwrap_err();
    assert_eq!(code(&err), "NoSuchBucket");
}

#[tokio::test]
async fn test_access_point_policy_round_trip() {
    let endpoint = start_server().await;
    let (control, _) = with_bucket(&endpoint).await;
    control
        .create_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .bucket("data")
        .send()
        .await
        .unwrap();

    let err = control
        .get_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .unwrap_err();
    assert_eq!(code(&err), "NoSuchAccessPointPolicy");

    let policy = format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"AWS":"arn:aws:iam::000000000000:role/owner"}},"Action":"s3:GetObject","Resource":"{ARN}/object/private/*"}}]}}"#
    );
    control
        .put_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .policy(&policy)
        .send()
        .await
        .expect("put_access_point_policy");
    let got = control
        .get_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .expect("get_access_point_policy");
    assert_eq!(got.policy(), Some(policy.as_str()));

    let err = control
        .put_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .policy("not a policy")
        .send()
        .await
        .unwrap_err();
    assert_eq!(code(&err), "MalformedPolicy");

    control
        .delete_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .expect("delete_access_point_policy");
    let err = control
        .get_access_point_policy()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .unwrap_err();
    assert_eq!(code(&err), "NoSuchAccessPointPolicy");
}

#[tokio::test]
async fn test_objects_through_access_point_arn_and_alias() {
    let endpoint = start_server().await;
    let (control, s3) = with_bucket(&endpoint).await;
    let alias = control
        .create_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .bucket("data")
        .send()
        .await
        .unwrap()
        .alias()
        .unwrap()
        .to_string();

    s3.put_object()
        .bucket(ARN)
        .key("private/a.txt")
        .body(ByteStream::from_static(b"hello"))
        .send()
        .await
        .expect("put_object through the ARN");
    let listed = s3
        .list_objects_v2()
        .bucket(ARN)
        .prefix("private/")
        .send()
        .await
        .expect("list_objects_v2 through the ARN");
    assert_eq!(listed.key_count(), Some(1));

    let path_style = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&config(&endpoint).await)
            .force_path_style(true)
            .build(),
    );
    for (client, bucket) in [
        (&path_style, alias.as_str()),
        (&path_style, "data"),
        (&s3, ARN),
    ] {
        let got = client
            .get_object()
            .bucket(bucket)
            .key("private/a.txt")
            .send()
            .await
            .unwrap_or_else(|e| panic!("get_object via {bucket}: {e}"));
        assert_eq!(got.body.collect().await.unwrap().into_bytes(), "hello");
    }

    control
        .delete_access_point()
        .account_id(ACCOUNT)
        .name("owner-1")
        .send()
        .await
        .unwrap();
    let err = s3
        .get_object()
        .bucket(ARN)
        .key("private/a.txt")
        .send()
        .await
        .unwrap_err();
    assert_eq!(
        err.as_service_error()
            .and_then(|e| aws_sdk_s3::error::ProvideErrorMetadata::code(e)),
        Some("NoSuchAccessPoint")
    );
}
