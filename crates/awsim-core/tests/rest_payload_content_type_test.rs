//! A REST service's `Content-Type` describes its payload, not the wire protocol.
//!
//! The gateway let content-type sniffing override the service's declared protocol, so an S3
//! `PutObject` carrying `Content-Type: application/json` was parsed and answered as
//! restJson1: the S3 headers it needs were dropped and the response was not XML, which
//! surfaced to SDKs as HTTP 500.

use awsim_core::protocol::{self, Protocol};
use axum::http::{HeaderMap, HeaderValue};
use bytes::Bytes;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_static(value));
    }
    map
}

#[test]
fn rest_xml_service_keeps_its_protocol_for_a_json_payload() {
    let h = headers(&[("content-type", "application/json")]);
    let body = Bytes::from_static(b"{}");
    assert_eq!(
        protocol::effective_protocol(Protocol::RestXml, &h, &body),
        Protocol::RestXml
    );
}

#[test]
fn rest_json_service_keeps_its_protocol_for_an_xml_payload() {
    let h = headers(&[("content-type", "application/xml")]);
    let body = Bytes::from_static(b"<a/>");
    assert_eq!(
        protocol::effective_protocol(Protocol::RestJson1, &h, &body),
        Protocol::RestJson1
    );
}

#[test]
fn amz_target_still_selects_aws_json() {
    let h = headers(&[
        ("content-type", "application/x-amz-json-1.0"),
        ("x-amz-target", "DynamoDB_20120810.ListTables"),
    ]);
    let body = Bytes::from_static(b"{}");
    assert_eq!(
        protocol::effective_protocol(Protocol::RestJson1, &h, &body),
        Protocol::AwsJson1_0
    );
}

#[test]
fn ec2_query_service_keeps_its_envelope() {
    let h = headers(&[("content-type", "application/x-www-form-urlencoded")]);
    let body = Bytes::from_static(b"Action=DescribeVpcs&Version=2016-11-15");
    assert_eq!(
        protocol::effective_protocol(Protocol::Ec2Query, &h, &body),
        Protocol::Ec2Query
    );
}

#[test]
fn missing_content_type_falls_back_to_the_declaration() {
    let body = Bytes::new();
    assert_eq!(
        protocol::effective_protocol(Protocol::RestXml, &HeaderMap::new(), &body),
        Protocol::RestXml
    );
}
