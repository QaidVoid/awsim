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
///
/// Recognised "magic" keys on the output value:
///   * `__raw_body` — base64-encoded bytes that become the response body verbatim.
///   * `__content_type` — overrides the `content-type` header.
///   * `__status_code` — overrides the HTTP status (defaults to 200).
///   * `__headers` — extra response headers `{ "Header-Name": "value", ... }`.
///
/// When `__raw_body` is absent, the entire output is JSON-encoded as the body
/// (after stripping the magic keys above).
pub fn serialize_response(output: &Value, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let status = extract_status(output);
    let mut headers = HeaderMap::new();
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());

    if let Some(raw_b64) = output.get("__raw_body").and_then(Value::as_str) {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(raw_b64)
            .unwrap_or_default();
        let content_type = output
            .get("__content_type")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        headers.insert("content-type", content_type.parse().unwrap());
        apply_extra_headers(&mut headers, output);
        return (status, headers, Bytes::from(data));
    }

    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    apply_extra_headers(&mut headers, output);

    let body_value = strip_magic_keys(output);
    let body = serde_json::to_vec(&body_value).unwrap_or_default();
    (status, headers, Bytes::from(body))
}

fn extract_status(output: &Value) -> StatusCode {
    output
        .get("__status_code")
        .and_then(Value::as_u64)
        .and_then(|n| u16::try_from(n).ok())
        .and_then(|n| StatusCode::from_u16(n).ok())
        .unwrap_or(StatusCode::OK)
}

fn apply_extra_headers(headers: &mut HeaderMap, output: &Value) {
    let Some(extra) = output.get("__headers").and_then(Value::as_object) else {
        return;
    };
    for (name, value) in extra {
        let Some(s) = value.as_str() else { continue };
        if let (Ok(k), Ok(v)) = (
            axum::http::header::HeaderName::from_bytes(name.as_bytes()),
            axum::http::HeaderValue::from_str(s),
        ) {
            headers.insert(k, v);
        }
    }
}

fn strip_magic_keys(output: &Value) -> Value {
    let Some(map) = output.as_object() else {
        return output.clone();
    };
    let mut cleaned = serde_json::Map::with_capacity(map.len());
    for (k, v) in map {
        if matches!(
            k.as_str(),
            "__raw_body" | "__content_type" | "__status_code" | "__headers"
        ) {
            continue;
        }
        cleaned.insert(k.clone(), v.clone());
    }
    Value::Object(cleaned)
}

/// Serialize a JSON error response.
pub fn serialize_error(error: &AwsError, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let mut body = serde_json::Map::new();
    body.insert("__type".to_string(), Value::String(error.code.clone()));
    body.insert("message".to_string(), Value::String(error.message.clone()));
    if let Some(extras) = &error.extras {
        for (k, v) in extras.as_ref() {
            body.insert(k.clone(), v.clone());
        }
    }
    let body = serde_json::to_vec(&Value::Object(body)).unwrap_or_default();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        "application/x-amz-json-1.0".parse().unwrap(),
    );
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());
    headers.insert("x-amzn-errortype", error.code.parse().unwrap());
    (error.status, headers, Bytes::from(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn applies_extra_headers_and_status() {
        let output = json!({
            "Foo": "bar",
            "__status_code": 202u64,
            "__headers": { "X-Amz-Function-Error": "Handled" },
        });
        let (status, headers, body) = serialize_response(&output, "req-1");
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(
            headers
                .get("x-amz-function-error")
                .and_then(|v| v.to_str().ok()),
            Some("Handled")
        );
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["Foo"], json!("bar"));
        assert!(parsed.get("__status_code").is_none());
        assert!(parsed.get("__headers").is_none());
    }

    #[test]
    fn defaults_to_ok_when_no_status_provided() {
        let (status, _headers, body) = serialize_response(&json!({"Hello": "world"}), "req-2");
        assert_eq!(status, StatusCode::OK);
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed, json!({"Hello": "world"}));
    }

    #[test]
    fn raw_body_path_respects_status_and_extra_headers() {
        use base64::Engine;
        let payload = b"hello";
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
        let output = json!({
            "__raw_body": encoded,
            "__content_type": "text/plain",
            "__status_code": 201u64,
            "__headers": { "X-Custom": "yes" },
        });
        let (status, headers, body) = serialize_response(&output, "req-3");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body.as_ref(), payload);
        assert_eq!(
            headers.get("content-type").and_then(|v| v.to_str().ok()),
            Some("text/plain")
        );
        assert_eq!(
            headers.get("x-custom").and_then(|v| v.to_str().ok()),
            Some("yes")
        );
    }
}
