//! CBOR protocol detection and rpcv2 path routing.
//!
//! Two dialects have to work: the legacy `application/x-amz-cbor-1.1`
//! form the AWS SDK for Java v1 sends to DynamoDB and Kinesis, and the
//! Smithy `rpc-v2-cbor` form routed by path. Before this, AWSim spoke
//! neither, so a Java v1 client failed on its first call.

use awsim_core::protocol::{
    Protocol, detect_protocol, is_cbor_request, operation_from_rpcv2_path, service_from_rpcv2_path,
};
use axum::http::HeaderMap;
use bytes::Bytes;

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.insert(
            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    h
}

#[test]
fn legacy_java_v1_dialect_is_detected() {
    // X-Amz-Target plus the CBOR content type. The JSON branch would
    // otherwise claim this, since it also carries X-Amz-Target.
    let h = headers(&[
        ("content-type", "application/x-amz-cbor-1.1"),
        ("x-amz-target", "DynamoDB_20120810.PutItem"),
    ]);
    assert_eq!(
        detect_protocol(&h, &Bytes::new()),
        Some(Protocol::RpcV2Cbor)
    );
}

#[test]
fn smithy_marker_is_detected() {
    let h = headers(&[
        ("content-type", "application/cbor"),
        ("smithy-protocol", "rpc-v2-cbor"),
    ]);
    assert_eq!(
        detect_protocol(&h, &Bytes::new()),
        Some(Protocol::RpcV2Cbor)
    );
}

#[test]
fn plain_cbor_content_type_is_detected() {
    let h = headers(&[("content-type", "application/cbor")]);
    assert!(is_cbor_request(&h));
}

/// The regression that matters most: existing clients must not be
/// rerouted onto the CBOR path.
#[test]
fn existing_protocols_are_unaffected() {
    let json10 = headers(&[
        ("content-type", "application/x-amz-json-1.0"),
        ("x-amz-target", "DynamoDB_20120810.PutItem"),
    ]);
    assert_eq!(
        detect_protocol(&json10, &Bytes::new()),
        Some(Protocol::AwsJson1_0)
    );

    let json11 = headers(&[
        ("content-type", "application/x-amz-json-1.1"),
        ("x-amz-target", "AWSEvents.PutRule"),
    ]);
    assert_eq!(
        detect_protocol(&json11, &Bytes::new()),
        Some(Protocol::AwsJson1_1)
    );

    let query = headers(&[("content-type", "application/x-www-form-urlencoded")]);
    assert_eq!(
        detect_protocol(&query, &Bytes::from_static(b"Action=GetCallerIdentity")),
        Some(Protocol::AwsQuery)
    );

    let rest = headers(&[("content-type", "application/json")]);
    assert_eq!(
        detect_protocol(&rest, &Bytes::new()),
        Some(Protocol::RestJson1)
    );

    let xml = headers(&[("content-type", "application/xml")]);
    assert_eq!(
        detect_protocol(&xml, &Bytes::new()),
        Some(Protocol::RestXml)
    );
}

#[test]
fn rpcv2_path_yields_service_and_operation() {
    let path = "/service/DynamoDB_20120810/operation/GetItem";
    assert_eq!(
        service_from_rpcv2_path(path).as_deref(),
        Some("DynamoDB_20120810")
    );
    assert_eq!(operation_from_rpcv2_path(path).as_deref(), Some("GetItem"));
}

#[test]
fn non_rpcv2_paths_are_ignored() {
    for path in [
        "/",
        "/mybucket/key",
        "/service/OnlyService",
        "/notservice/X/operation/Y",
        "/service//operation/Y",
        "/service/X/notoperation/Y",
    ] {
        assert!(
            operation_from_rpcv2_path(path).is_none(),
            "`{path}` should not parse as an rpcv2 operation path"
        );
    }
}

#[test]
fn cbor_protocol_reports_its_content_type() {
    assert_eq!(
        Protocol::RpcV2Cbor.response_content_type(),
        "application/cbor"
    );
    assert!(Protocol::RpcV2Cbor.is_cbor());
    assert!(!Protocol::RpcV2Cbor.is_json());
    assert!(!Protocol::RpcV2Cbor.is_xml());
}
