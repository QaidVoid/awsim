//! Browser POST form upload behavior contract.
//!
//! `PostObject` is the HTML form upload path and has no AWS SDK operation,
//! so these tests drive it with a raw `multipart/form-data` request and
//! then read the result back through the S3 SDK. They assert the object
//! actually lands, that the `${filename}` template and success response
//! shaping behave, and that the POST policy is enforced.

use base64::Engine as _;

async fn s3_client() -> (aws_sdk_s3::Client, String) {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    (aws_sdk_s3::Client::new(&config), endpoint)
}

/// Build a `multipart/form-data` body from ordered (name, filename, content)
/// triples and return the body bytes alongside the boundary.
fn multipart_body(fields: &[(&str, Option<&str>, &str)]) -> (Vec<u8>, String) {
    let boundary = "awsimconformanceboundary".to_string();
    let mut body = Vec::new();
    for (name, filename, content) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(f) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\n")
                    .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(content.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (body, boundary)
}

/// POST a form body to the bucket root, routing to S3 by Host header since
/// the raw request carries no SigV4 credential scope.
async fn post_form(
    endpoint: &str,
    bucket: &str,
    body: Vec<u8>,
    boundary: &str,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{endpoint}/{bucket}"))
        .header("host", "s3.us-east-1.localhost")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .expect("send POST form")
}

fn b64_policy(policy: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(policy)
}

#[tokio::test]
async fn post_object_stores_object_and_returns_201() {
    let (client, endpoint) = s3_client().await;
    client
        .create_bucket()
        .bucket("postbucket")
        .send()
        .await
        .expect("create bucket");

    let policy = b64_policy(
        "{\"expiration\":\"2300-01-01T00:00:00.000Z\",\
          \"conditions\":[{\"bucket\":\"postbucket\"},\
          [\"starts-with\",\"$key\",\"uploads/\"],\
          [\"content-length-range\",1,1048576]]}",
    );
    let (body, boundary) = multipart_body(&[
        ("key", None, "uploads/${filename}"),
        ("policy", None, &policy),
        ("success_action_status", None, "201"),
        ("file", Some("report.txt"), "hello from the form"),
    ]);

    let resp = post_form(&endpoint, "postbucket", body, &boundary).await;
    assert_eq!(resp.status().as_u16(), 201);
    let text = resp.text().await.expect("body");
    assert!(
        text.contains("<Key>uploads/report.txt</Key>"),
        "body: {text}"
    );

    // The object must be readable through the SDK with the expanded key.
    let got = client
        .get_object()
        .bucket("postbucket")
        .key("uploads/report.txt")
        .send()
        .await
        .expect("get object");
    let bytes = got.body.collect().await.expect("collect").into_bytes();
    assert_eq!(&bytes[..], b"hello from the form");
}

#[tokio::test]
async fn post_object_defaults_to_204() {
    let (client, endpoint) = s3_client().await;
    client
        .create_bucket()
        .bucket("quietbucket")
        .send()
        .await
        .expect("create bucket");

    let policy = b64_policy("{\"conditions\":[]}");
    let (body, boundary) = multipart_body(&[
        ("key", None, "silent.txt"),
        ("policy", None, &policy),
        ("file", Some("silent.txt"), "no ceremony"),
    ]);

    let resp = post_form(&endpoint, "quietbucket", body, &boundary).await;
    assert_eq!(resp.status().as_u16(), 204);

    let got = client
        .get_object()
        .bucket("quietbucket")
        .key("silent.txt")
        .send()
        .await
        .expect("get object");
    let bytes = got.body.collect().await.expect("collect").into_bytes();
    assert_eq!(&bytes[..], b"no ceremony");
}

#[tokio::test]
async fn post_object_enforces_policy_conditions() {
    let (client, endpoint) = s3_client().await;
    client
        .create_bucket()
        .bucket("policedbucket")
        .send()
        .await
        .expect("create bucket");

    // The key does not satisfy the starts-with condition: expect 403.
    let policy = b64_policy(
        "{\"expiration\":\"2300-01-01T00:00:00.000Z\",\
          \"conditions\":[[\"starts-with\",\"$key\",\"uploads/\"]]}",
    );
    let (body, boundary) = multipart_body(&[
        ("key", None, "elsewhere/x.txt"),
        ("policy", None, &policy),
        ("file", Some("x.txt"), "blocked"),
    ]);
    let resp = post_form(&endpoint, "policedbucket", body, &boundary).await;
    assert_eq!(resp.status().as_u16(), 403);

    // A file larger than the content-length-range maximum: expect 400.
    let policy = b64_policy("{\"conditions\":[[\"content-length-range\",1,4]]}");
    let (body, boundary) = multipart_body(&[
        ("key", None, "uploads/big.txt"),
        ("policy", None, &policy),
        (
            "file",
            Some("big.txt"),
            "this body is far too large for the range",
        ),
    ]);
    let resp = post_form(&endpoint, "policedbucket", body, &boundary).await;
    assert_eq!(resp.status().as_u16(), 400);
}
