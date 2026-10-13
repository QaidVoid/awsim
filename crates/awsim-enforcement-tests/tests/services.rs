//! Per-service E2E tests: real SDK calls flow through the gateway and
//! the test asserts Allow / AccessDenied outcomes. The harness in
//! `lib.rs` registers every service whose handler declares
//! `iam_action`, and we toggle enforcement at runtime so resources
//! created during setup persist into the enforced phase.

use std::sync::Arc;

use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType,
};
use awsim_enforcement_tests::{
    ALLOW_ALL_S3, ALLOW_DDB_TABLE, ALLOW_GETOBJECT, ALLOW_IAM_RO, ALLOW_LAMBDA_INVOKE,
    ALLOW_SECRET_READ, ALLOW_SNS_PUBLISH, ALLOW_SQS_SEND, bootstrap_user, dynamodb_client,
    iam_client, lambda_client, make_sdk_config, s3_client, sdk_err_is_access_denied,
    secretsmanager_client, sns_client, sqs_client, start_server_unenforced,
};

// -- DynamoDB -----------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ddb_alice_can_put_get_only_her_table() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");

    let admin_ddb = dynamodb_client(&admin_cfg);
    admin_ddb
        .create_table()
        .table_name("widgets")
        .billing_mode(BillingMode::PayPerRequest)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(KeyType::Hash)
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect("create_table widgets");

    admin_ddb
        .create_table()
        .table_name("other")
        .billing_mode(BillingMode::PayPerRequest)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(KeyType::Hash)
                .build()
                .unwrap(),
        )
        .send()
        .await
        .expect("create_table other");

    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceDdb".into(), ALLOW_DDB_TABLE.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let ddb = dynamodb_client(&cfg);

    ddb.put_item()
        .table_name("widgets")
        .item("id", AttributeValue::S("1".into()))
        .send()
        .await
        .expect("PutItem widgets");

    let err = ddb
        .put_item()
        .table_name("other")
        .item("id", AttributeValue::S("1".into()))
        .send()
        .await
        .expect_err("PutItem other must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "PutItem on unauthorized table must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- SQS ----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sqs_alice_send_only_work_queue() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin_sqs = sqs_client(&admin_cfg);

    let q = admin_sqs
        .create_queue()
        .queue_name("work")
        .send()
        .await
        .expect("create_queue work");
    let work_url = q.queue_url.expect("queue url");
    let other = admin_sqs
        .create_queue()
        .queue_name("other")
        .send()
        .await
        .expect("create_queue other");
    let other_url = other.queue_url.expect("other url");

    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceSqs".into(), ALLOW_SQS_SEND.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let sqs = sqs_client(&cfg);

    sqs.send_message()
        .queue_url(&work_url)
        .message_body("hello")
        .send()
        .await
        .expect("SendMessage work");

    let err = sqs
        .send_message()
        .queue_url(&other_url)
        .message_body("hello")
        .send()
        .await
        .expect_err("SendMessage other must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "SendMessage to unauthorized queue must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- SNS ----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sns_alice_publish_only_alerts_topic() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin_sns = sns_client(&admin_cfg);

    let alerts = admin_sns
        .create_topic()
        .name("alerts")
        .send()
        .await
        .expect("create_topic alerts");
    let alerts_arn = alerts.topic_arn.expect("alerts arn");
    let other = admin_sns
        .create_topic()
        .name("noise")
        .send()
        .await
        .expect("create_topic noise");
    let other_arn = other.topic_arn.expect("noise arn");

    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceSns".into(), ALLOW_SNS_PUBLISH.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let sns = sns_client(&cfg);

    sns.publish()
        .topic_arn(&alerts_arn)
        .message("hi")
        .send()
        .await
        .expect("Publish alerts");

    let err = sns
        .publish()
        .topic_arn(&other_arn)
        .message("hi")
        .send()
        .await
        .expect_err("Publish noise must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "Publish to unauthorized topic must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- Lambda -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lambda_alice_invoke_unauthorized_function_denied() {
    // We don't deploy a real function (would need a runtime). The
    // gateway runs the IAM check before the handler; an Invoke on a
    // function the principal isn't authorized for must fail with
    // AccessDenied regardless of whether the function exists.
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceLambda".into(), ALLOW_LAMBDA_INVOKE.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let lambda = lambda_client(&cfg);

    let err = lambda
        .invoke()
        .function_name("other")
        .send()
        .await
        .expect_err("Invoke other must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "Invoke unauthorized function must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- Secrets Manager ----------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn secretsmanager_alice_can_create_and_read() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceSecrets".into(), ALLOW_SECRET_READ.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let sm = secretsmanager_client(&cfg);

    sm.create_secret()
        .name("api_key")
        .secret_string("topsecret")
        .send()
        .await
        .expect("CreateSecret");
    let got = sm
        .get_secret_value()
        .secret_id("api_key")
        .send()
        .await
        .expect("GetSecretValue");
    assert_eq!(got.secret_string.as_deref(), Some("topsecret"));

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn secretsmanager_no_policy_implicit_deny() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin_iam = iam_client(&admin_cfg);
    admin_iam
        .create_user()
        .user_name("eve")
        .send()
        .await
        .unwrap();
    let k = admin_iam
        .create_access_key()
        .user_name("eve")
        .send()
        .await
        .unwrap()
        .access_key
        .unwrap();

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &k.access_key_id, &k.secret_access_key);
    let sm = secretsmanager_client(&cfg);

    let err = sm
        .get_secret_value()
        .secret_id("api_key")
        .send()
        .await
        .expect_err("GetSecretValue without policy must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "implicit deny must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- IAM as a target service --------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn iam_target_alice_readonly_can_list_but_not_create() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceIamRo".into(), ALLOW_IAM_RO.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let iam_sdk = iam_client(&cfg);

    iam_sdk
        .list_users()
        .send()
        .await
        .expect("ListUsers allowed by ALLOW_IAM_RO");

    let err = iam_sdk
        .create_user()
        .user_name("bob")
        .send()
        .await
        .expect_err("CreateUser must fail for read-only alice");
    assert!(
        sdk_err_is_access_denied(&err),
        "CreateUser must be AccessDenied for read-only principal: {err}"
    );

    srv.shutdown().await;
}

// -- S3 deeper coverage -------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_bucket_arn_does_not_grant_object_access() {
    // ListObjectsV2 is authorized as `s3:ListBucket` on the bucket ARN.
    // The policy below grants that plus GetObject on the bucket ARN.
    // The GetObject must still be denied because object access requires
    // the object-level ARN.
    let bucket_only = r#"{
        "Version":"2012-10-17",
        "Statement":[{
            "Effect":"Allow",
            "Action":["s3:ListBucket","s3:GetObject"],
            "Resource":"arn:aws:s3:::test-bucket"
        }]
    }"#;

    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    s3_client(&admin_cfg)
        .create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .expect("create_bucket");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceBucketOnly".into(), bucket_only.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let s3 = s3_client(&cfg);

    s3.list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .expect("ListObjectsV2 allowed by bucket-ARN policy");

    let err = s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
        .expect_err("GetObject must fail without object-ARN allow");
    assert!(
        sdk_err_is_access_denied(&err),
        "GetObject with bucket-only ARN must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_operations_are_checked_as_iam_actions() {
    let policy = r#"{
        "Version":"2012-10-17",
        "Statement":[
            {"Effect":"Allow","Action":"s3:ListAllMyBuckets","Resource":"*"},
            {"Effect":"Allow","Action":"s3:ListBucket","Resource":"arn:aws:s3:::test-bucket"},
            {"Effect":"Allow","Action":["s3:GetObject","s3:DeleteObject"],
             "Resource":"arn:aws:s3:::test-bucket/*"},
            {"Effect":"Allow","Action":"s3:PutObject","Resource":"arn:aws:s3:::test-bucket/copies/*"}
        ]
    }"#;

    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin_s3 = s3_client(&admin_cfg);
    for bucket in ["test-bucket", "other-bucket"] {
        admin_s3
            .create_bucket()
            .bucket(bucket)
            .send()
            .await
            .expect("create_bucket");
        admin_s3
            .put_object()
            .bucket(bucket)
            .key("a.txt")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"a"))
            .send()
            .await
            .expect("put_object");
    }
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceS3".into(), policy.into())],
    )
    .await;

    srv.set_enforcement(true);

    let s3 = s3_client(&make_sdk_config(port, &ak, &sk));
    s3.list_buckets().send().await.expect("ListBuckets");
    s3.head_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .expect("HeadBucket");
    s3.list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .expect("ListObjectsV2");
    s3.head_object()
        .bucket("test-bucket")
        .key("a.txt")
        .send()
        .await
        .expect("HeadObject");
    s3.copy_object()
        .copy_source("test-bucket/a.txt")
        .bucket("test-bucket")
        .key("copies/a.txt")
        .send()
        .await
        .expect("CopyObject within allowed source and target");

    let err = s3
        .copy_object()
        .copy_source("other-bucket/a.txt")
        .bucket("test-bucket")
        .key("copies/b.txt")
        .send()
        .await
        .expect_err("CopyObject from an unreadable source");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    let err = s3
        .list_objects_v2()
        .bucket("other-bucket")
        .send()
        .await
        .expect_err("ListObjectsV2 on another bucket");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    let delete = |keys: &[&str]| {
        aws_sdk_s3::types::Delete::builder()
            .set_objects(Some(
                keys.iter()
                    .map(|k| {
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key(*k)
                            .build()
                            .unwrap()
                    })
                    .collect(),
            ))
            .build()
            .unwrap()
    };
    let err = s3
        .delete_objects()
        .bucket("other-bucket")
        .delete(delete(&["a.txt"]))
        .send()
        .await
        .expect_err("DeleteObjects on another bucket's keys");
    assert!(sdk_err_is_access_denied(&err), "{err}");
    s3.delete_objects()
        .bucket("test-bucket")
        .delete(delete(&["a.txt", "copies/a.txt"]))
        .send()
        .await
        .expect("DeleteObjects with s3:DeleteObject on each key");

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_cross_bucket_access_denied() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin_s3 = s3_client(&admin_cfg);
    admin_s3
        .create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .expect("create test-bucket");
    admin_s3
        .create_bucket()
        .bucket("other-bucket")
        .send()
        .await
        .expect("create other-bucket");

    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceGetTest".into(), ALLOW_GETOBJECT.into())],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let s3 = s3_client(&cfg);

    let err = s3
        .get_object()
        .bucket("other-bucket")
        .key("foo.txt")
        .send()
        .await
        .expect_err("cross-bucket GetObject must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "cross-bucket GetObject must be AccessDenied: {err}"
    );

    srv.shutdown().await;
}

// -- SCP across services ------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scp_deny_all_iam_blocks_admin_in_target_account() {
    let scp_deny_iam = r#"{
        "Version":"2012-10-17",
        "Statement":[{
            "Effect":"Deny",
            "Action":"iam:*",
            "Resource":"*"
        }]
    }"#;

    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = awsim_enforcement_tests::start_server_with_scp(
        false,
        iam.clone(),
        Some(awsim_enforcement_tests::with_scp(scp_deny_iam)),
    )
    .await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "carol",
        &[
            ("CarolFullS3".into(), ALLOW_ALL_S3.into()),
            (
                "CarolIamFull".into(),
                r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"iam:*","Resource":"*"}]}"#
                    .into(),
            ),
        ],
    )
    .await;

    srv.set_enforcement(true);

    let cfg = make_sdk_config(port, &ak, &sk);
    let iam_sdk = iam_client(&cfg);

    let err = iam_sdk
        .create_user()
        .user_name("dave")
        .send()
        .await
        .expect_err("CreateUser must be denied by SCP");
    assert!(
        sdk_err_is_access_denied(&err),
        "SCP Deny iam:* must yield AccessDenied: {err}"
    );

    srv.shutdown().await;
}
