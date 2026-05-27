//! Legacy AWS Query protocol (form-encoded request, XML response) for
//! SQS. Older SDKs predating SQS's 2022 switch to JSON 1.0 send
//! requests as `application/x-www-form-urlencoded`; the simulator's
//! gateway detects them via Content-Type and routes through the Query
//! parser before handing the JSON-shaped input to the SQS handler.
//!
//! This test exercises the same `parse_request` → `handle` →
//! `serialize_response` chain the gateway uses, so a regression in
//! either the parser or the encoder shows up here without needing to
//! spin up the full HTTP stack.

use awsim_core::protocol::{ParsedRequest, parse_request, serialize_response};
use awsim_core::{Protocol, RequestContext, ServiceHandler};
use awsim_sqs::SqsService;
use axum::http::{HeaderMap, HeaderValue, Method, Uri};
use bytes::Bytes;

fn header_map() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        "content-type",
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    h
}

fn ctx() -> RequestContext {
    RequestContext::new("sqs", "us-east-1")
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop_clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    fn noop(_: *const ()) {}
    fn noop_raw_waker() -> RawWaker {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(f);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}

fn parse_form(body: &str) -> ParsedRequest {
    parse_request(
        Protocol::AwsQuery,
        &Method::POST,
        &Uri::from_static("/"),
        &header_map(),
        &Bytes::from(body.to_string()),
        &[],
    )
    .expect("Query parse succeeds")
}

#[test]
fn create_queue_round_trips_through_query_protocol() {
    let body = "Action=CreateQueue&Version=2012-11-05&QueueName=legacy-q";
    let parsed = parse_form(body);
    assert_eq!(parsed.operation, "CreateQueue");
    assert_eq!(parsed.input["QueueName"], "legacy-q");

    let svc = SqsService::new();
    let output =
        block_on(svc.handle(&parsed.operation, parsed.input, &ctx())).expect("CreateQueue ok");
    assert!(output["QueueUrl"].as_str().unwrap().contains("legacy-q"));

    let (status, headers, xml) =
        serialize_response(Protocol::AwsQuery, &parsed.operation, &output, "req-1");
    assert_eq!(status.as_u16(), 200);
    assert!(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("xml"),
        "Query response must be XML"
    );
    let body_str = std::str::from_utf8(&xml).unwrap();
    assert!(
        body_str.contains("<CreateQueueResponse"),
        "missing response wrapper: {body_str}"
    );
    assert!(
        body_str.contains("<QueueUrl"),
        "missing QueueUrl element: {body_str}"
    );
    assert!(
        body_str.contains("legacy-q"),
        "missing queue name: {body_str}"
    );
}

#[test]
fn send_message_via_query_protocol_returns_xml_with_md5() {
    let svc = SqsService::new();

    // Bootstrap: create the queue first (also exercising the Query path).
    let create = parse_form("Action=CreateQueue&Version=2012-11-05&QueueName=msg-q");
    let create_out = block_on(svc.handle(&create.operation, create.input, &ctx())).unwrap();
    let queue_url = create_out["QueueUrl"].as_str().unwrap().to_string();

    let send_body = format!(
        "Action=SendMessage&Version=2012-11-05&QueueUrl={}&MessageBody=hello",
        urlencode(&queue_url)
    );
    let send = parse_form(&send_body);
    assert_eq!(send.operation, "SendMessage");
    assert_eq!(send.input["QueueUrl"], queue_url.as_str());
    assert_eq!(send.input["MessageBody"], "hello");

    let send_out = block_on(svc.handle(&send.operation, send.input, &ctx())).expect("SendMessage");
    assert!(send_out["MD5OfMessageBody"].as_str().is_some());

    let (_, _, xml) = serialize_response(Protocol::AwsQuery, &send.operation, &send_out, "req-2");
    let body = std::str::from_utf8(&xml).unwrap();
    assert!(body.contains("<SendMessageResponse"), "{body}");
    assert!(body.contains("<MD5OfMessageBody"), "{body}");
    assert!(body.contains("<MessageId"), "{body}");
}

#[test]
fn unknown_action_returns_validation_error() {
    let parsed = parse_form("Action=FakeAction&Version=2012-11-05");
    let svc = SqsService::new();
    let err = block_on(svc.handle(&parsed.operation, parsed.input, &ctx())).unwrap_err();
    assert_eq!(err.code, "UnknownOperationException");
}

#[test]
fn missing_action_param_is_rejected_at_parse_time() {
    let parsed = parse_request(
        Protocol::AwsQuery,
        &Method::POST,
        &Uri::from_static("/"),
        &header_map(),
        &Bytes::from_static(b"Version=2012-11-05&QueueName=anon"),
        &[],
    );
    let err = parsed.expect_err("missing Action must fail");
    assert_eq!(err.code, "MissingAction");
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
