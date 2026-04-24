use axum::http::{HeaderMap, StatusCode};
use bytes::Bytes;
use serde_json::Value;

use crate::error::AwsError;

use super::ParsedRequest;

/// Parse an awsJson (1.0/1.1) request.
///
/// Operation is extracted from the `X-Amz-Target` header:
/// Format: `ServicePrefix_Version.OperationName` (e.g., `DynamoDB_20120810.GetItem`)
pub fn parse_request(headers: &HeaderMap, body: &Bytes) -> Result<ParsedRequest, AwsError> {
    let target = headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AwsError::bad_request("MissingHeader", "Missing X-Amz-Target header"))?;

    let operation = target
        .split('.')
        .next_back()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidHeader",
                format!("Invalid X-Amz-Target format: {target}"),
            )
        })?
        .to_string();

    let input = if body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(body).map_err(|e| {
            AwsError::bad_request("SerializationException", format!("Invalid JSON body: {e}"))
        })?
    };

    Ok(ParsedRequest { operation, input })
}

/// Serialize a successful JSON response.
pub fn serialize_response(output: &Value, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    if let Some(raw_b64) = output.get("__raw_body").and_then(Value::as_str) {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(raw_b64)
            .unwrap_or_default();
        let content_type = output
            .get("__content_type")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        let mut headers = HeaderMap::new();
        headers.insert("content-type", content_type.parse().unwrap());
        headers.insert("x-amzn-requestid", request_id.parse().unwrap());
        return (StatusCode::OK, headers, Bytes::from(data));
    }

    let body = serde_json::to_vec(output).unwrap_or_default();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());
    (StatusCode::OK, headers, Bytes::from(body))
}

/// Serialize a JSON error response.
pub fn serialize_error(error: &AwsError, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let body = serde_json::json!({
        "__type": error.code,
        "message": error.message,
    });
    let body = serde_json::to_vec(&body).unwrap_or_default();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());
    headers.insert("x-amzn-errortype", error.code.parse().unwrap());
    (error.status, headers, Bytes::from(body))
}
