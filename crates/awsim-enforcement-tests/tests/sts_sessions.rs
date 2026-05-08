//! End-to-end coverage for STS-issued temp credentials under IAM
//! enforcement. Before the session store landed, every request
//! signed with `ASIA…` keys was denied because the IAM principal
//! lookup couldn't resolve them — this exercise pins down the
//! happy path so that regression can't return.

use std::sync::Arc;

use aws_credential_types::Credentials;
use awsim_enforcement_tests::{
    bootstrap_user, iam_client, make_sdk_config, sdk_err_is_access_denied, start_server_unenforced,
};

const TRUST_ANYONE: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"AWS": "*"},
    "Action": "sts:AssumeRole"
  }]
}"#;

const ALLOW_DESCRIBE_TABLE: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": "dynamodb:DescribeTable",
    "Resource": "arn:aws:dynamodb:us-east-1:000000000000:table/chat_sessions"
  }]
}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assumed_role_temp_creds_resolve_under_enforcement() {
    // Single long-lived server: enforcement starts off so we can
    // bootstrap IAM and DDB state, then flips on for the actual
    // exercise. Restarting between would lose DDB state since the
    // service is recreated per-server.
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");

    // Caller user — needs valid IAM creds for AssumeRole to resolve a
    // principal once enforcement flips on.
    let (caller_ak, caller_sk) = bootstrap_user(&iam_client(&admin_cfg), "caller", &[]).await;

    // Role with DescribeTable on a specific ARN.
    let admin_iam = iam_client(&admin_cfg);
    admin_iam
        .create_role()
        .role_name("AppAuthRole")
        .assume_role_policy_document(TRUST_ANYONE)
        .send()
        .await
        .unwrap();
    admin_iam
        .put_role_policy()
        .role_name("AppAuthRole")
        .policy_name("Inline1")
        .policy_document(ALLOW_DESCRIBE_TABLE)
        .send()
        .await
        .unwrap();

    // Pre-create the table — also using admin creds since enforcement is off.
    let ddb_admin = aws_sdk_dynamodb::Client::new(&admin_cfg);
    ddb_admin
        .create_table()
        .table_name("chat_sessions")
        .billing_mode(aws_sdk_dynamodb::types::BillingMode::PayPerRequest)
        .attribute_definitions(
            aws_sdk_dynamodb::types::AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .key_schema(
            aws_sdk_dynamodb::types::KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                .build()
                .unwrap(),
        )
        .send()
        .await
        .unwrap();

    // Flip enforcement on without restarting so DDB state survives.
    srv.set_enforcement(true);
    let caller_cfg = make_sdk_config(port, &caller_ak, &caller_sk);

    // 1. AssumeRole as the caller. This issues temp creds and records
    //    a session into the shared store.
    let sts = aws_sdk_sts::Client::new(&caller_cfg);
    let assumed = sts
        .assume_role()
        .role_arn("arn:aws:iam::000000000000:role/AppAuthRole")
        .role_session_name("app-session")
        .send()
        .await
        .expect("AssumeRole");
    let creds = assumed.credentials.expect("Credentials");
    assert!(
        creds.access_key_id.starts_with("ASIA"),
        "temp access key must be ASIA-prefixed: {}",
        creds.access_key_id
    );

    // 2. DescribeTable signed with the temp creds — must succeed
    //    because the role's policy allows it on the matching ARN.
    let temp_cfg = aws_config::SdkConfig::builder()
        .behavior_version(aws_config::BehaviorVersion::latest())
        .endpoint_url(format!("http://127.0.0.1:{port}"))
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(Credentials::new(
                creds.access_key_id.clone(),
                creds.secret_access_key.clone(),
                Some(creds.session_token.clone()),
                None,
                "test",
            )),
        )
        .build();
    let ddb_caller = aws_sdk_dynamodb::Client::new(&temp_cfg);
    let ok = ddb_caller
        .describe_table()
        .table_name("chat_sessions")
        .send()
        .await
        .expect("DescribeTable on the policy-matching table must succeed under temp creds");
    assert_eq!(
        ok.table.and_then(|t| t.table_name).as_deref(),
        Some("chat_sessions")
    );

    // 3. DescribeTable on a *different* table — implicit deny since
    //    the role's policy is scoped to one ARN. Confirms ARN
    //    matching actually runs against the assumed-role principal.
    let denied = ddb_caller
        .describe_table()
        .table_name("not_in_policy")
        .send()
        .await
        .expect_err("DescribeTable on an out-of-scope table must be denied");
    assert!(
        sdk_err_is_access_denied(&denied),
        "out-of-scope table must yield AccessDenied; got {denied:?}"
    );

    // 4. GetCallerIdentity reports the assumed-role ARN, not the
    //    synthetic iam:user/ASIA… shape that older STS code emitted.
    let sts_caller = aws_sdk_sts::Client::new(&temp_cfg);
    let id = sts_caller.get_caller_identity().send().await.unwrap();
    assert_eq!(
        id.arn.as_deref(),
        Some("arn:aws:sts::000000000000:assumed-role/AppAuthRole/app-session")
    );

    srv.shutdown().await;
}
