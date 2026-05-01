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
}

impl Protocol {
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::AwsJson1_0 | Self::AwsJson1_1 | Self::RestJson1 => "application/x-amz-json-1.0",
            Self::RestXml | Self::AwsQuery | Self::Ec2Query => "application/xml",
        }
    }

    pub fn is_json(&self) -> bool {
        matches!(self, Self::AwsJson1_0 | Self::AwsJson1_1 | Self::RestJson1)
    }

    pub fn is_xml(&self) -> bool {
        matches!(self, Self::RestXml | Self::AwsQuery | Self::Ec2Query)
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
    /// e.g., PUT /{Bucket}?versioning → PutBucketVersioning
    pub required_query_param: Option<&'static str>,
}

/// Detect which protocol an incoming request uses.
pub fn detect_protocol(headers: &HeaderMap, body: &Bytes) -> Option<Protocol> {
    // Check X-Amz-Target header → awsJson
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

    // Check form-encoded → awsQuery or ec2Query
    if content_type.contains("x-www-form-urlencoded") {
        let body_str = std::str::from_utf8(body).unwrap_or("");
        if body_str.contains("Action=") {
            return Some(Protocol::AwsQuery);
        }
    }

    // Check JSON content type → restJson1
    if content_type.contains("json") {
        return Some(Protocol::RestJson1);
    }

    // Check XML content type → restXml
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
        Protocol::RestJson1 => rest::parse_json_request(method, uri, body, routes),
        Protocol::RestXml => rest::parse_xml_request(method, uri, headers, body, routes),
    }
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
    }
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
        Protocol::AwsQuery | Protocol::Ec2Query | Protocol::RestXml => {
            query::serialize_error(error, request_id)
        }
    }
}
