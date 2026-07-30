//! A trailing slash in a request path must not hide a route.
//!
//! AWS models are inconsistent about it: Route53 spells
//! `ChangeResourceRecordSets` as `/rrset/` and `ListResourceRecordSets`
//! as `/rrset`, and Backup spells three list paths with a slash the
//! others lack. An SDK sends exactly what the model says, so a router
//! that only matched the bare form left those operations unreachable
//! with `UnknownOperationException`.
//!
//! The one place the slash carries meaning is S3, where it marks a
//! folder object, so that has to keep working.

use awsim_core::protocol::{Protocol, RouteDefinition, parse_request};
use axum::http::{HeaderMap, Method, Uri};
use bytes::Bytes;

fn routes() -> Vec<RouteDefinition> {
    vec![
        RouteDefinition {
            method: "POST",
            path_pattern: "/2013-04-01/hostedzone/{Id}/rrset",
            operation: "ChangeResourceRecordSets",
            required_query_param: None,
        },
        RouteDefinition {
            method: "GET",
            path_pattern: "/backup/plans",
            operation: "ListBackupPlans",
            required_query_param: None,
        },
        // S3-style: a greedy key that legitimately ends in a slash.
        RouteDefinition {
            method: "PUT",
            path_pattern: "/{Bucket}/{Key+}",
            operation: "PutObject",
            required_query_param: None,
        },
    ]
}

fn resolve(method: &str, path: &str) -> Result<(String, serde_json::Value), String> {
    let uri: Uri = path.parse().expect("uri");
    let method: Method = method.parse().expect("method");
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/xml".parse().unwrap());
    parse_request(
        Protocol::RestXml,
        &method,
        &uri,
        &headers,
        &Bytes::from_static(b"<Root/>"),
        &routes(),
    )
    .map(|p| (p.operation, p.input))
    .map_err(|e| e.code)
}

#[test]
fn a_deep_path_matches_with_a_trailing_slash() {
    let (op, input) = resolve("POST", "/2013-04-01/hostedzone/Z123/rrset/")
        .expect("the model spells this path with a trailing slash");
    assert_eq!(op, "ChangeResourceRecordSets");
    assert_eq!(input["Id"], "Z123");
}

#[test]
fn the_bare_form_still_matches() {
    let (op, _) = resolve("POST", "/2013-04-01/hostedzone/Z123/rrset").expect("bare form");
    assert_eq!(op, "ChangeResourceRecordSets");
}

#[test]
fn a_two_segment_path_matches_with_a_trailing_slash() {
    let (op, _) = resolve("GET", "/backup/plans/").expect("Backup spells this with a slash");
    assert_eq!(op, "ListBackupPlans");
}

/// The retry must not eat a slash that is part of an S3 object key.
#[test]
fn an_s3_folder_marker_keeps_its_slash() {
    let (op, input) = resolve("PUT", "/my-bucket/myfolder/").expect("folder marker");
    assert_eq!(op, "PutObject");
    assert_eq!(
        input["Key"], "myfolder/",
        "the trailing slash is what makes this a folder marker"
    );
}

#[test]
fn a_path_that_matches_nothing_still_fails() {
    assert_eq!(
        resolve("GET", "/nope/nothing/here/"),
        Err("UnknownOperationException".to_string())
    );
}
