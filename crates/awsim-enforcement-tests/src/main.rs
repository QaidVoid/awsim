use std::sync::Arc;

use awsim_enforcement_tests::{
    ALLOW_ALL_S3, ALLOW_GETOBJECT, DENY_PUTOBJECT, bootstrap_user, iam_client, make_sdk_config,
    s3_client, sdk_err_is_access_denied, start_server_enforced, start_server_unenforced,
};

struct Report {
    passes: Vec<String>,
    failures: Vec<(String, String)>,
}

impl Report {
    fn new() -> Self {
        Self {
            passes: vec![],
            failures: vec![],
        }
    }
    fn pass(&mut self, name: &str) {
        println!("PASS: {name}");
        self.passes.push(name.to_string());
    }
    fn fail(&mut self, name: &str, why: String) {
        println!("FAIL: {name}: {why}");
        self.failures.push((name.to_string(), why));
    }
}

async fn scenario_enforced(report: &mut Report) {
    let iam_service = Arc::new(awsim_iam::IamService::new());

    let (admin_srv, admin_port) = start_server_unenforced(iam_service.clone()).await;
    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let admin_iam = iam_client(&admin_cfg);

    let (alice_ak, alice_sk) = bootstrap_user(
        &admin_iam,
        "alice",
        &[("AliceGetObject".to_string(), ALLOW_GETOBJECT.to_string())],
    )
    .await;

    admin_iam
        .create_user()
        .user_name("bob")
        .send()
        .await
        .expect("create_user bob");
    let bob_keys = admin_iam
        .create_access_key()
        .user_name("bob")
        .send()
        .await
        .expect("create_access_key bob")
        .access_key
        .unwrap();
    let bob_ak = bob_keys.access_key_id;
    let bob_sk = bob_keys.secret_access_key;

    let (carol_ak, carol_sk) = bootstrap_user(
        &admin_iam,
        "carol",
        &[
            ("CarolAllowS3".to_string(), ALLOW_ALL_S3.to_string()),
            ("CarolDenyPut".to_string(), DENY_PUTOBJECT.to_string()),
        ],
    )
    .await;

    admin_srv.shutdown().await;

    let (srv, port) = start_server_enforced(iam_service.clone()).await;

    let alice_cfg = make_sdk_config(port, &alice_ak, &alice_sk);
    let alice_s3 = s3_client(&alice_cfg);

    match alice_s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
    {
        Ok(_) => report.pass("alice_get_object_allowed"),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.fail(
                    "alice_get_object_allowed",
                    "expected non-AccessDenied, got 403".into(),
                );
            } else {
                report.pass("alice_get_object_allowed");
            }
        }
    }

    match alice_s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await
    {
        Ok(_) => report.fail(
            "alice_put_object_denied",
            "expected AccessDenied, got Ok".into(),
        ),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.pass("alice_put_object_denied");
            } else {
                report.fail(
                    "alice_put_object_denied",
                    format!("expected AccessDenied, got {e:?}"),
                );
            }
        }
    }

    match alice_s3.delete_bucket().bucket("test-bucket").send().await {
        Ok(_) => report.fail(
            "alice_delete_bucket_denied",
            "expected AccessDenied, got Ok".into(),
        ),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.pass("alice_delete_bucket_denied");
            } else {
                report.fail(
                    "alice_delete_bucket_denied",
                    format!("expected AccessDenied, got {e:?}"),
                );
            }
        }
    }

    let bob_cfg = make_sdk_config(port, &bob_ak, &bob_sk);
    let bob_s3 = s3_client(&bob_cfg);
    match bob_s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
    {
        Ok(_) => report.fail(
            "bob_get_object_implicit_deny",
            "expected AccessDenied, got Ok".into(),
        ),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.pass("bob_get_object_implicit_deny");
            } else {
                report.fail(
                    "bob_get_object_implicit_deny",
                    format!("expected AccessDenied, got {e:?}"),
                );
            }
        }
    }

    let carol_cfg = make_sdk_config(port, &carol_ak, &carol_sk);
    let carol_s3 = s3_client(&carol_cfg);

    match carol_s3
        .get_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .send()
        .await
    {
        Ok(_) => report.pass("carol_get_object_allowed"),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.fail(
                    "carol_get_object_allowed",
                    "expected allow (allow-all), got AccessDenied".into(),
                );
            } else {
                report.pass("carol_get_object_allowed");
            }
        }
    }

    match carol_s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await
    {
        Ok(_) => report.fail(
            "carol_explicit_deny_overrides_allow",
            "expected AccessDenied from explicit Deny, got Ok".into(),
        ),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.pass("carol_explicit_deny_overrides_allow");
            } else {
                report.fail(
                    "carol_explicit_deny_overrides_allow",
                    format!("expected AccessDenied, got {e:?}"),
                );
            }
        }
    }

    srv.shutdown().await;
}

async fn scenario_unenforced(report: &mut Report) {
    let iam_service = Arc::new(awsim_iam::IamService::new());
    let (admin_srv, admin_port) = start_server_unenforced(iam_service.clone()).await;

    let admin_cfg = make_sdk_config(admin_port, "admin", "admin");
    let admin_iam = iam_client(&admin_cfg);

    admin_iam
        .create_user()
        .user_name("nobody")
        .send()
        .await
        .expect("create_user");
    let keys = admin_iam
        .create_access_key()
        .user_name("nobody")
        .send()
        .await
        .expect("create_access_key")
        .access_key
        .unwrap();

    let nobody_cfg = make_sdk_config(admin_port, &keys.access_key_id, &keys.secret_access_key);
    let nobody_s3 = s3_client(&nobody_cfg);

    let name = "unenforced_put_no_block";
    match nobody_s3
        .put_object()
        .bucket("test-bucket")
        .key("foo.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"x"))
        .send()
        .await
    {
        Ok(_) => report.pass(name),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.fail(name, "enforcement off but got AccessDenied".into());
            } else {
                report.pass(name);
            }
        }
    }

    let name = "unenforced_delete_bucket_no_block";
    match nobody_s3.delete_bucket().bucket("test-bucket").send().await {
        Ok(_) => report.pass(name),
        Err(e) => {
            if sdk_err_is_access_denied(&e) {
                report.fail(name, "enforcement off but got AccessDenied".into());
            } else {
                report.pass(name);
            }
        }
    }

    admin_srv.shutdown().await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .try_init()
        .ok();

    let mut report = Report::new();

    scenario_enforced(&mut report).await;
    scenario_unenforced(&mut report).await;

    println!();
    println!("--------------------------------------");
    println!(
        "Summary: {} passed, {} failed",
        report.passes.len(),
        report.failures.len()
    );
    if !report.failures.is_empty() {
        for (name, why) in &report.failures {
            println!("  - {name}: {why}");
        }
        std::process::exit(1);
    }
}
