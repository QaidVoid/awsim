use std::sync::Arc;

use awsim_enforcement_tests::{
    ALLOW_ALL_S3, ALLOW_GETOBJECT, DENY_PUTOBJECT, bootstrap_user, iam_client, make_sdk_config,
    s3_client, sdk_err_is_access_denied, start_server_enforced, start_server_enforced_with_scp,
    start_server_unenforced, with_scp,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enforced_alice_get_object_allowed_but_put_and_delete_denied() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (admin, admin_port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "alice",
        &[("AliceGetObject".into(), ALLOW_GETOBJECT.into())],
    )
    .await;
    admin.shutdown().await;

    let (srv, port) = start_server_enforced(iam).await;
    let cfg = make_sdk_config(port, &ak, &sk);
    let s3 = s3_client(&cfg);

    let got = s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await;
    if let Err(e) = &got {
        assert!(
            !sdk_err_is_access_denied(e),
            "GetObject must not be AccessDenied"
        );
    }

    let put = s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await;
    let err = put.expect_err("PutObject must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "PutObject must be AccessDenied"
    );

    let del = s3.delete_bucket().bucket("test-bucket").send().await;
    let err = del.expect_err("DeleteBucket must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "DeleteBucket must be AccessDenied"
    );

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enforced_bob_no_policy_implicit_deny() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (admin, admin_port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let admin_iam = iam_client(&admin_cfg);
    admin_iam
        .create_user()
        .user_name("bob")
        .send()
        .await
        .unwrap();
    let k = admin_iam
        .create_access_key()
        .user_name("bob")
        .send()
        .await
        .unwrap()
        .access_key
        .unwrap();
    admin.shutdown().await;

    let (srv, port) = start_server_enforced(iam).await;
    let cfg = make_sdk_config(port, &k.access_key_id, &k.secret_access_key);
    let s3 = s3_client(&cfg);

    let err = s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
        .expect_err("GetObject with no policy must fail");
    assert!(
        sdk_err_is_access_denied(&err),
        "implicit deny must yield AccessDenied"
    );

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enforced_explicit_deny_overrides_allow() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (admin, admin_port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "carol",
        &[
            ("CarolAllowS3".into(), ALLOW_ALL_S3.into()),
            ("CarolDenyPut".into(), DENY_PUTOBJECT.into()),
        ],
    )
    .await;
    admin.shutdown().await;

    let (srv, port) = start_server_enforced(iam).await;
    let cfg = make_sdk_config(port, &ak, &sk);
    let s3 = s3_client(&cfg);

    let err = s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await
        .expect_err("PutObject must fail due to explicit Deny");
    assert!(
        sdk_err_is_access_denied(&err),
        "explicit Deny must yield AccessDenied"
    );

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scp_explicit_deny_blocks_identity_allow() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (admin, admin_port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "dana",
        &[("DanaAllowAll".into(), ALLOW_ALL_S3.into())],
    )
    .await;
    admin.shutdown().await;

    let scp_deny_s3 = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Deny",
            "Action": "s3:GetObject",
            "Resource": "*"
        }]
    }"#;
    let scp = with_scp(scp_deny_s3);
    let (srv, port) = start_server_enforced_with_scp(iam, scp).await;
    let cfg = make_sdk_config(port, &ak, &sk);
    let s3 = s3_client(&cfg);

    let err = s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
        .expect_err("GetObject must fail under SCP Deny");
    assert!(
        sdk_err_is_access_denied(&err),
        "SCP explicit Deny must yield AccessDenied"
    );

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unenforced_permits_everything() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (admin, admin_port) = start_server_unenforced(iam.clone()).await;
    let cfg = make_sdk_config(admin_port, "test", "test");
    let s3 = s3_client(&cfg);

    if let Err(e) = s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await
    {
        assert!(
            !sdk_err_is_access_denied(&e),
            "enforcement off: must not be AccessDenied"
        );
    }

    if let Err(e) = s3.delete_bucket().bucket("test-bucket").send().await {
        assert!(
            !sdk_err_is_access_denied(&e),
            "enforcement off: must not be AccessDenied"
        );
    }

    admin.shutdown().await;
}
