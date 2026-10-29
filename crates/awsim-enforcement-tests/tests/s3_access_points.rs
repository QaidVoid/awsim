//! Requests through an S3 Access Point are authorized on the Access Point,
//! where the identity and Access Point policies apply, and on the bucket,
//! where the bucket policy applies, with `s3:DataAccessPointArn` set.

use std::sync::Arc;

use aws_sdk_s3::primitives::ByteStream;
use awsim_enforcement_tests::{
    bootstrap_user, iam_client, make_sdk_config, s3_client, sdk_err_is_access_denied,
    start_server_unenforced,
};

const POINT_ARN: &str = "arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1";

/// The bucket admits any request that arrives through one of the
/// account's Access Points, and no other.
const DELEGATE_TO_ACCESS_POINTS: &str = r#"{
    "Version":"2012-10-17",
    "Statement":[{
        "Effect":"Allow",
        "Principal":{"AWS":"*"},
        "Action":"s3:*",
        "Resource":["arn:aws:s3:::data","arn:aws:s3:::data/*"],
        "Condition":{"StringEquals":{"s3:DataAccessPointAccount":"000000000000"}}
    }]
}"#;

/// Work under `private/` through the one Access Point, never elsewhere.
fn owner_policy() -> String {
    format!(
        r#"{{
        "Version":"2012-10-17",
        "Statement":[
            {{"Effect":"Allow","Action":["s3:GetObject","s3:PutObject"],
              "Resource":"{POINT_ARN}/object/private/*"}},
            {{"Effect":"Deny","Action":"s3:*","Resource":"*",
              "Condition":{{"StringNotEquals":{{"s3:DataAccessPointArn":"{POINT_ARN}"}}}}}}
        ]
    }}"#
    )
}

/// Create the `owner-1` Access Point on `data` with a raw S3 Control call,
/// returning its alias.
async fn create_access_point(port: u16) -> String {
    let body = "<CreateAccessPointRequest xmlns=\"http://awss3control.amazonaws.com/doc/2018-08-20/\">\
                <Bucket>data</Bucket></CreateAccessPointRequest>";
    let response = reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{port}/v20180820/accesspoint/owner-1"
        ))
        .header(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=admin/20260101/us-east-1/s3/aws4_request, \
             SignedHeaders=host, Signature=x",
        )
        .header("x-amz-account-id", "000000000000")
        .body(body)
        .send()
        .await
        .expect("CreateAccessPoint");
    assert!(response.status().is_success(), "{}", response.status());
    let xml = response.text().await.unwrap();
    let start = xml.find("<Alias>").expect("alias") + "<Alias>".len();
    let end = xml.find("</Alias>").expect("alias end");
    xml[start..end].to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_access_point_confines_an_owner_to_its_prefix() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin = s3_client(&make_sdk_config(port, "admin", "admin"));
    admin.create_bucket().bucket("data").send().await.unwrap();
    admin
        .put_bucket_policy()
        .bucket("data")
        .policy(DELEGATE_TO_ACCESS_POINTS)
        .send()
        .await
        .unwrap();
    let alias = create_access_point(port).await;
    let (ak, sk) = bootstrap_user(
        &iam_client(&make_sdk_config(port, "admin", "admin")),
        "owner",
        &[("Owner".into(), owner_policy())],
    )
    .await;

    srv.set_enforcement(true);
    let s3 = s3_client(&make_sdk_config(port, &ak, &sk));

    s3.put_object()
        .bucket(&alias)
        .key("private/a.txt")
        .body(ByteStream::from_static(b"hello"))
        .send()
        .await
        .expect("PutObject through the Access Point");
    let got = s3
        .get_object()
        .bucket(&alias)
        .key("private/a.txt")
        .send()
        .await
        .expect("GetObject through the Access Point");
    assert_eq!(got.body.collect().await.unwrap().into_bytes(), "hello");

    let err = s3
        .get_object()
        .bucket(&alias)
        .key("other/b.txt")
        .send()
        .await
        .expect_err("outside the prefix");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    let err = s3
        .get_object()
        .bucket("data")
        .key("private/a.txt")
        .send()
        .await
        .expect_err("addressed to the bucket, not the Access Point");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_access_point_needs_the_bucket_policy_too() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin = s3_client(&make_sdk_config(port, "admin", "admin"));
    admin.create_bucket().bucket("data").send().await.unwrap();
    let alias = create_access_point(port).await;
    let (ak, sk) = bootstrap_user(
        &iam_client(&make_sdk_config(port, "admin", "admin")),
        "owner",
        &[("Owner".into(), owner_policy())],
    )
    .await;

    srv.set_enforcement(true);
    let s3 = s3_client(&make_sdk_config(port, &ak, &sk));
    let err = s3
        .get_object()
        .bucket(&alias)
        .key("private/a.txt")
        .send()
        .await
        .expect_err("no bucket policy admits the Access Point");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_access_point_policy_grants_what_the_identity_does_not() {
    let iam = Arc::new(awsim_iam::IamService::new());
    let (srv, port) = start_server_unenforced(iam.clone()).await;
    let admin_cfg = make_sdk_config(port, "admin", "admin");
    let admin = s3_client(&admin_cfg);
    admin.create_bucket().bucket("data").send().await.unwrap();
    admin
        .put_bucket_policy()
        .bucket("data")
        .policy(DELEGATE_TO_ACCESS_POINTS)
        .send()
        .await
        .unwrap();
    admin
        .put_object()
        .bucket("data")
        .key("private/a.txt")
        .body(ByteStream::from_static(b"hello"))
        .send()
        .await
        .unwrap();
    let alias = create_access_point(port).await;
    let only_through_the_point = format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Deny","Action":"s3:*","Resource":"*",
            "Condition":{{"StringNotEquals":{{"s3:DataAccessPointArn":"{POINT_ARN}"}}}}}}]}}"#
    );
    let (ak, sk) = bootstrap_user(
        &iam_client(&admin_cfg),
        "owner",
        &[("Owner".into(), only_through_the_point)],
    )
    .await;
    let point_policy = format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow",
            "Principal":{{"AWS":"arn:aws:iam::000000000000:user/owner"}},
            "Action":"s3:GetObject","Resource":"{POINT_ARN}/object/private/*"}}]}}"#
    );
    let policy_body = format!(
        "<PutAccessPointPolicyRequest><Policy>{}</Policy></PutAccessPointPolicyRequest>",
        point_policy
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('"', "&quot;")
    );
    let response = reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{port}/v20180820/accesspoint/owner-1/policy"
        ))
        .header(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=admin/20260101/us-east-1/s3/aws4_request, \
             SignedHeaders=host, Signature=x",
        )
        .header("x-amz-account-id", "000000000000")
        .body(policy_body)
        .send()
        .await
        .expect("PutAccessPointPolicy");
    assert!(response.status().is_success(), "{}", response.status());

    srv.set_enforcement(true);
    let s3 = s3_client(&make_sdk_config(port, &ak, &sk));
    s3.get_object()
        .bucket(&alias)
        .key("private/a.txt")
        .send()
        .await
        .expect("granted by the Access Point policy");
    let err = s3
        .get_object()
        .bucket(&alias)
        .key("other/a.txt")
        .send()
        .await
        .expect_err("outside what the Access Point policy grants");
    assert!(sdk_err_is_access_denied(&err), "{err}");

    srv.shutdown().await;
}
