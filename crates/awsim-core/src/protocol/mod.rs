pub mod cbor;
pub mod eventstream;
pub mod json;
pub mod query;
pub mod rest;

use axum::http::{HeaderMap, Method, Uri};
use bytes::Bytes;
use serde_json::Value;

use crate::error::AwsError;

/// AWS API protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    AwsJson1_0,
    AwsJson1_1,
    RestJson1,
    RestXml,
    AwsQuery,
    Ec2Query,
    /// AWS CBOR, covering both the legacy `application/x-amz-cbor-1.1`
    /// dialect (X-Amz-Target dispatch) and Smithy `rpcv2Cbor` (path
    /// dispatch). One variant serves both because they differ only in
    /// how the operation is named, not in how the body is encoded.
    RpcV2Cbor,
}

impl Protocol {
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::AwsJson1_0 | Self::AwsJson1_1 | Self::RestJson1 => "application/x-amz-json-1.0",
            Self::RestXml | Self::AwsQuery | Self::Ec2Query => "application/xml",
            Self::RpcV2Cbor => "application/cbor",
        }
    }

    pub fn is_json(&self) -> bool {
        matches!(self, Self::AwsJson1_0 | Self::AwsJson1_1 | Self::RestJson1)
    }

    pub fn is_xml(&self) -> bool {
        matches!(self, Self::RestXml | Self::AwsQuery | Self::Ec2Query)
    }

    /// Whether bodies for this protocol are CBOR rather than text.
    pub fn is_cbor(&self) -> bool {
        matches!(self, Self::RpcV2Cbor)
    }
}

/// Parsed AWS request ready for dispatch to a service handler.
#[derive(Debug)]
pub struct ParsedRequest {
    pub operation: String,
    pub input: Value,
}

/// Route definition for REST-style services.
#[derive(Debug, Clone)]
pub struct RouteDefinition {
    pub method: &'static str,
    pub path_pattern: &'static str,
    pub operation: &'static str,
    /// For S3-style query parameter disambiguation.
    /// e.g., PUT /{Bucket}?versioning -> PutBucketVersioning
    pub required_query_param: Option<&'static str>,
}

/// Detect which protocol an incoming request uses.
pub fn detect_protocol(headers: &HeaderMap, body: &Bytes) -> Option<Protocol> {
    // CBOR first: the legacy dialect also carries X-Amz-Target, so the
    // JSON branch below would claim it otherwise.
    if is_cbor_request(headers) {
        return Some(Protocol::RpcV2Cbor);
    }

    // Check X-Amz-Target header -> awsJson
    if let Some(target) = headers.get("x-amz-target") {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content_type.contains("x-amz-json-1.0") {
            return Some(Protocol::AwsJson1_0);
        }
        // Default to 1.1 if X-Amz-Target present but content-type doesn't specify 1.0
        let _ = target;
        return Some(Protocol::AwsJson1_1);
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Check form-encoded -> awsQuery or ec2Query
    if content_type.contains("x-www-form-urlencoded") {
        let body_str = std::str::from_utf8(body).unwrap_or("");
        if body_str.contains("Action=") {
            return Some(Protocol::AwsQuery);
        }
    }

    // Check JSON content type -> restJson1
    if content_type.contains("json") {
        return Some(Protocol::RestJson1);
    }

    // Check XML content type -> restXml
    if content_type.contains("xml") {
        return Some(Protocol::RestXml);
    }

    // For REST protocols without explicit content-type (GET/HEAD/DELETE with no body),
    // we determine protocol from the service's declared protocol
    None
}

/// Parse a request based on the detected protocol.
pub fn parse_request(
    protocol: Protocol,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    routes: &[RouteDefinition],
) -> Result<ParsedRequest, AwsError> {
    match protocol {
        Protocol::AwsJson1_0 | Protocol::AwsJson1_1 => json::parse_request(headers, body),
        Protocol::AwsQuery | Protocol::Ec2Query => query::parse_request(body),
        Protocol::RestJson1 => rest::parse_json_request(method, uri, headers, body, routes),
        Protocol::RestXml => rest::parse_xml_request(method, uri, headers, body, routes),
        Protocol::RpcV2Cbor => parse_cbor_request(uri, headers, body),
    }
}

/// True when the request body is CBOR.
///
/// Recognises the Smithy `smithy-protocol: rpc-v2-cbor` marker and both
/// spellings of the CBOR content type.
pub fn is_cbor_request(headers: &HeaderMap) -> bool {
    if headers
        .get("smithy-protocol")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("rpc-v2-cbor"))
    {
        return true;
    }
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("application/cbor") || ct.contains("x-amz-cbor"))
}

/// Resolve the operation for a CBOR request.
///
/// Legacy clients send `X-Amz-Target: Service.Operation`. Smithy clients
/// route by path at `/service/{Service}/operation/{Operation}`.
fn parse_cbor_request(
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<ParsedRequest, AwsError> {
    let input = cbor::decode(body)?;

    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok()) {
        let operation = target.rsplit('.').next().unwrap_or(target).to_string();
        return Ok(ParsedRequest { operation, input });
    }

    if let Some(operation) = operation_from_rpcv2_path(uri.path()) {
        return Ok(ParsedRequest { operation, input });
    }

    Err(AwsError::bad_request(
        "UnknownOperationException",
        "CBOR request carried neither X-Amz-Target nor an rpcv2 operation path",
    ))
}

/// Extract the operation from `/service/{Service}/operation/{Operation}`.
pub fn operation_from_rpcv2_path(path: &str) -> Option<String> {
    let mut parts = path.trim_matches('/').split('/');
    if parts.next()? != "service" {
        return None;
    }
    // The service segment must be present and non-empty: an operation
    // with no service cannot be dispatched anywhere.
    if parts.next()?.is_empty() {
        return None;
    }
    if parts.next()? != "operation" {
        return None;
    }
    let operation = parts.next()?;
    if operation.is_empty() {
        return None;
    }
    Some(operation.to_string())
}

/// Extract the service name from `/service/{Service}/operation/{Operation}`.
pub fn service_from_rpcv2_path(path: &str) -> Option<String> {
    let mut parts = path.trim_matches('/').split('/');
    if parts.next()? != "service" {
        return None;
    }
    let service = parts.next()?;
    if service.is_empty() {
        return None;
    }
    Some(service.to_string())
}

/// Serialize a successful response based on protocol.
pub fn serialize_response(
    protocol: Protocol,
    operation: &str,
    output: &Value,
    request_id: &str,
) -> (axum::http::StatusCode, HeaderMap, Bytes) {
    // Streaming responses (Bedrock ConverseStream / InvokeModelWith
    // ResponseStream, etc.) tag their output with an event-stream
    // marker. Detect it before falling through to the per-protocol
    // JSON/XML/Query encoders so the SDK gets the binary frames it
    // expects under `application/vnd.amazon.eventstream`.
    if let Some(body) = eventstream::try_encode(output) {
        let mut headers = HeaderMap::new();
        if let Ok(v) = "application/vnd.amazon.eventstream".parse() {
            headers.insert(axum::http::header::CONTENT_TYPE, v);
        }
        if let Ok(v) = request_id.parse() {
            headers.insert("x-amzn-requestid", v);
        }
        return (axum::http::StatusCode::OK, headers, Bytes::from(body));
    }

    match protocol {
        Protocol::AwsJson1_0 | Protocol::AwsJson1_1 | Protocol::RestJson1 => {
            json::serialize_response(output, request_id)
        }
        Protocol::AwsQuery | Protocol::Ec2Query => {
            query::serialize_response(operation, output, request_id)
        }
        Protocol::RestXml => rest::serialize_xml_response(output, request_id),
        Protocol::RpcV2Cbor => serialize_cbor_response(output, request_id),
    }
}

/// Serialize a successful CBOR response.
fn serialize_cbor_response(
    output: &Value,
    request_id: &str,
) -> (axum::http::StatusCode, HeaderMap, Bytes) {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/cbor"),
    );
    headers.insert(
        "smithy-protocol",
        axum::http::HeaderValue::from_static("rpc-v2-cbor"),
    );
    if let Ok(v) = request_id.parse() {
        headers.insert("x-amzn-requestid", v);
    }
    let body = cbor::encode(output).unwrap_or_default();
    (axum::http::StatusCode::OK, headers, Bytes::from(body))
}

/// Serialize an error response based on protocol.
pub fn serialize_error(
    protocol: Protocol,
    error: &AwsError,
    request_id: &str,
) -> (axum::http::StatusCode, HeaderMap, Bytes) {
    match protocol {
        Protocol::AwsJson1_0 | Protocol::AwsJson1_1 | Protocol::RestJson1 => {
            json::serialize_error(error, request_id)
        }
        Protocol::AwsQuery | Protocol::Ec2Query => query::serialize_error(error, request_id),
        Protocol::RestXml => rest::serialize_error(error, request_id),
        Protocol::RpcV2Cbor => serialize_cbor_error(error, request_id),
    }
}

/// Serialize an error for a CBOR client.
///
/// Same status and `x-amzn-RequestId` as the JSON path, with the body
/// CBOR-encoded, so a client branching on the error code behaves
/// identically over either encoding.
fn serialize_cbor_error(
    error: &AwsError,
    request_id: &str,
) -> (axum::http::StatusCode, HeaderMap, Bytes) {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/cbor"),
    );
    headers.insert(
        "smithy-protocol",
        axum::http::HeaderValue::from_static("rpc-v2-cbor"),
    );
    if let Ok(v) = request_id.parse() {
        headers.insert("x-amzn-requestid", v);
    }
    let body = cbor::encode_error(&error.code, &error.message);
    (error.status, headers, Bytes::from(body))
}
