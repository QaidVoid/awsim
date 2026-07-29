//! ec2Query uses a different response envelope from awsQuery.
//!
//! EC2 puts result fields directly under `<{Action}Response>` with a
//! lowercase `<requestId>`, and wraps errors in `<Response><Errors>`.
//! AWSim emitted the awsQuery `<{Action}Result>` / `<ErrorResponse>`
//! shapes, so every EC2 response parsed as empty: `aws ec2 describe-vpcs`
//! printed nothing even with a VPC present.

use awsim_core::error::AwsError;
use awsim_core::protocol::{self, Protocol};
use serde_json::json;

fn body(protocol: Protocol, operation: &str, output: &serde_json::Value) -> String {
    let (_, _, bytes) = protocol::serialize_response(protocol, operation, output, "req-1");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

#[test]
fn ec2_success_has_no_result_wrapper() {
    let out = json!({ "vpcSet": { "item": [{ "vpcId": "vpc-1" }] } });
    let xml = body(Protocol::Ec2Query, "DescribeVpcs", &out);

    assert!(
        xml.contains("<DescribeVpcsResponse xmlns=\"http://ec2.amazonaws.com/doc/2016-11-15/\">"),
        "EC2 uses its own namespace: {xml}"
    );
    assert!(
        !xml.contains("<DescribeVpcsResult>"),
        "ec2Query has no Result wrapper: {xml}"
    );
    assert!(
        !xml.contains("<ResponseMetadata>"),
        "ec2Query carries a bare requestId, not ResponseMetadata: {xml}"
    );
    assert!(xml.contains("<requestId>req-1</requestId>"), "{xml}");
    assert!(xml.contains("<vpcId>vpc-1</vpcId>"), "{xml}");
}

/// awsQuery services must keep the envelope they already had.
#[test]
fn aws_query_keeps_the_result_wrapper() {
    let out = json!({ "Topics": { "member": [] } });
    let xml = body(Protocol::AwsQuery, "ListTopics", &out);

    assert!(xml.contains("<ListTopicsResult>"), "{xml}");
    assert!(xml.contains("<ResponseMetadata>"), "{xml}");
}

#[test]
fn ec2_errors_use_the_response_errors_shape() {
    let err = AwsError::bad_request("InvalidVpcID.NotFound", "The vpc 'vpc-x' does not exist");
    let (status, _, bytes) = protocol::serialize_error(Protocol::Ec2Query, &err, "req-2");
    let xml = String::from_utf8(bytes.to_vec()).expect("utf8");

    assert_eq!(status, err.status);
    assert!(xml.contains("<Response>"), "{xml}");
    assert!(xml.contains("<Errors>"), "{xml}");
    assert!(xml.contains("<Code>InvalidVpcID.NotFound</Code>"), "{xml}");
    assert!(xml.contains("<RequestID>req-2</RequestID>"), "{xml}");
    assert!(
        !xml.contains("<ErrorResponse"),
        "that is the awsQuery shape: {xml}"
    );
}

/// A message quoting a caller-supplied name can contain markup.
#[test]
fn error_messages_are_xml_escaped() {
    let err = AwsError::bad_request("InvalidParameterValue", "bad name <a & b>");
    for protocol in [Protocol::AwsQuery, Protocol::Ec2Query] {
        let (_, _, bytes) = protocol::serialize_error(protocol, &err, "req-3");
        let xml = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(
            xml.contains("bad name &lt;a &amp; b&gt;"),
            "{protocol:?}: {xml}"
        );
    }
}
