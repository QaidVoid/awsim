//! A list has to survive every encoding a service answers in.
//!
//! CloudWatch speaks JSON (and CBOR) to a modern SDK and the query
//! protocol to an older one, all from one handler output. Handlers shape
//! lists for XML, where each item needs its own `<member>` element, so
//! the JSON and CBOR encoders have to strip that wrapper again. Without
//! that, `aws cloudwatch list-metrics` died inside botocore's parser
//! while the same data rendered correctly as XML.

use awsim_core::protocol::{self, Protocol};
use serde_json::{Value, json};

fn output() -> Value {
    json!({
        "Metrics": { "member": [
            { "Namespace": "App", "MetricName": "Hits",
              "Dimensions": { "member": [{ "Name": "Service", "Value": "auth" }] } },
            { "Namespace": "App", "MetricName": "Errors",
              "Dimensions": { "member": [] } },
        ]},
    })
}

fn body(protocol: Protocol) -> Vec<u8> {
    let (_, _, bytes) = protocol::serialize_response(protocol, "ListMetrics", &output(), "req-1");
    bytes.to_vec()
}

#[test]
fn json_drops_the_member_wrapper() {
    let parsed: Value = serde_json::from_slice(&body(Protocol::AwsJson1_0)).expect("json");

    let metrics = parsed["Metrics"]
        .as_array()
        .unwrap_or_else(|| panic!("Metrics should be a bare array: {parsed}"));
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0]["Dimensions"][0]["Name"], "Service");
    assert_eq!(
        metrics[1]["Dimensions"],
        json!([]),
        "an empty list still has to be a list: {parsed}"
    );
}

#[test]
fn cbor_drops_the_member_wrapper() {
    let value: ciborium::value::Value =
        ciborium::from_reader(body(Protocol::RpcV2Cbor).as_slice()).expect("cbor");
    let ciborium::value::Value::Map(entries) = value else {
        panic!("expected a CBOR map");
    };
    let metrics = entries
        .iter()
        .find(|(k, _)| matches!(k, ciborium::value::Value::Text(t) if t == "Metrics"))
        .map(|(_, v)| v)
        .expect("Metrics");
    assert!(
        matches!(metrics, ciborium::value::Value::Array(items) if items.len() == 2),
        "Metrics should be a bare CBOR array"
    );
}

/// The XML protocols keep the wrapper, which is the whole reason the
/// handler emits it.
#[test]
fn query_keeps_each_item_in_its_own_element() {
    let xml = String::from_utf8(body(Protocol::AwsQuery)).expect("utf8");
    assert_eq!(
        xml.matches("<member>").count(),
        3,
        "two metrics plus one dimension: {xml}"
    );
    assert!(xml.contains("<MetricName>Hits</MetricName>"), "{xml}");
}

/// A structure that happens to hold a single field is not a list.
#[test]
fn a_lone_object_field_named_member_is_not_unwrapped() {
    let out = json!({ "Group": { "member": { "Name": "alice" } } });
    let (_, _, bytes) = protocol::serialize_response(Protocol::AwsJson1_0, "Get", &out, "req-2");
    let parsed: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(parsed["Group"]["member"]["Name"], "alice");
}
