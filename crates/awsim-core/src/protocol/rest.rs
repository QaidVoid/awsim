use axum::http::{HeaderMap, Method, StatusCode, Uri};
use base64::Engine as _;
use bytes::Bytes;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::AwsError;

use super::{ParsedRequest, RouteDefinition};

/// Parse a restJson1 request.
///
/// Operation is determined by matching HTTP method + URI path against route definitions.
pub fn parse_json_request(
    method: &Method,
    uri: &Uri,
    body: &Bytes,
    routes: &[RouteDefinition],
) -> Result<ParsedRequest, AwsError> {
    let path = uri.path();
    let query_string = uri.query().unwrap_or("");

    let (operation, path_params) = match_route(method.as_str(), path, query_string, routes)?;

    let mut input = if body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(body).map_err(|e| {
            AwsError::bad_request("SerializationException", format!("Invalid JSON body: {e}"))
        })?
    };

    // Merge path parameters into input
    if let Value::Object(ref mut map) = input {
        for (key, value) in path_params {
            map.insert(key, Value::String(value));
        }
        // Merge query parameters
        for (key, value) in parse_query_string(query_string) {
            map.entry(key).or_insert(Value::String(value));
        }
    }

    Ok(ParsedRequest {
        operation: operation.to_string(),
        input,
    })
}

/// Parse a restXml request (used by S3).
pub fn parse_xml_request(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    routes: &[RouteDefinition],
) -> Result<ParsedRequest, AwsError> {
    let path = uri.path();
    let query_string = uri.query().unwrap_or("");

    let (operation, path_params) = match_route(method.as_str(), path, query_string, routes)?;

    let mut input = if body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        // Only attempt XML parsing if the body actually looks like XML (starts
        // with '<').  Otherwise treat it as raw binary data and store it as
        // base64 in `__raw_body` so handlers like S3 PutObject can access it.
        let looks_like_xml = body.first().is_some_and(|&b| b == b'<');
        if looks_like_xml {
            match parse_xml_body(body) {
                Ok(v) => v,
                Err(_) => {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(body);
                    let mut map = serde_json::Map::new();
                    map.insert("__raw_body".to_string(), Value::String(encoded));
                    Value::Object(map)
                }
            }
        } else {
            // Non-XML body — always store as raw binary.
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(body);
            let mut map = serde_json::Map::new();
            map.insert("__raw_body".to_string(), Value::String(encoded));
            Value::Object(map)
        }
    };

    // Merge path parameters
    if let Value::Object(ref mut map) = input {
        for (key, value) in path_params {
            map.insert(key, Value::String(value));
        }
        for (key, value) in parse_query_string(query_string) {
            map.entry(key).or_insert(Value::String(value));
        }
        // Extract relevant headers: all `x-amz-*` headers plus the standard
        // HTTP headers that S3 (and other restXml services) bind as request
        // input via `smithy.api#httpHeader` — Range and the four RFC 7232
        // conditional headers. The Smithy field name is the PascalCase of
        // the header (e.g. `If-Match` → `IfMatch`, `Range` → `Range`).
        for (name, value) in headers.iter() {
            let name_str = name.as_str();
            let is_amz = name_str.starts_with("x-amz-") && name_str != "x-amz-target";
            let is_http_input = matches!(
                name_str,
                "range"
                    | "if-match"
                    | "if-none-match"
                    | "if-modified-since"
                    | "if-unmodified-since"
                    | "content-md5"
                    | "content-encoding"
            );
            if (is_amz || is_http_input)
                && let Ok(v) = value.to_str()
            {
                let key = header_to_param_name(name_str);
                map.entry(key).or_insert(Value::String(v.to_string()));
            }
        }
    }

    Ok(ParsedRequest {
        operation: operation.to_string(),
        input,
    })
}

/// Result of matching an HTTP request against routes: the operation name and path parameters.
pub type RouteMatch<'a> = (&'a str, Vec<(String, String)>);

/// Match an HTTP request against route definitions.
/// Returns the operation name and extracted path parameters.
fn match_route<'a>(
    method: &str,
    path: &str,
    query_string: &str,
    routes: &'a [RouteDefinition],
) -> Result<RouteMatch<'a>, AwsError> {
    // Strip a trailing slash ONLY for bucket-level operations (paths like `/bucket/`).
    // Don't strip for object keys like `/bucket/folder/` — the trailing slash is
    // significant (it marks S3 "folder" objects).
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let path = if segments.len() <= 1 {
        // Bucket-level: `/bucket/` → `/bucket`
        let stripped = path.strip_suffix('/').unwrap_or(path);
        if stripped.is_empty() { "/" } else { stripped }
    } else {
        // Object-level: preserve trailing slash for folder markers
        path
    };

    let query_params: Vec<(String, String)> = parse_query_string(query_string);

    // Try routes with required_query_param first (more specific matches)
    let mut best_match: Option<(&str, Vec<(String, String)>)> = None;
    let mut best_specificity = 0;

    for route in routes {
        if !route.method.eq_ignore_ascii_case(method) {
            continue;
        }

        if let Some(path_params) = match_path_pattern(route.path_pattern, path) {
            let specificity = if route.required_query_param.is_some() {
                2
            } else {
                1
            };

            if let Some(required_param) = route.required_query_param {
                // This route requires a specific query parameter to be present
                if query_params.iter().any(|(k, _)| k == required_param)
                    && specificity > best_specificity
                {
                    best_match = Some((route.operation, path_params));
                    best_specificity = specificity;
                }
            } else if specificity > best_specificity
                || (specificity == best_specificity && best_match.is_none())
            {
                best_match = Some((route.operation, path_params));
                best_specificity = specificity;
            }
        }
    }

    best_match.ok_or_else(|| AwsError::unknown_operation(&format!("{method} {path}")))
}

/// Match a path pattern like "/2015-03-31/functions/{FunctionName}" against an actual path.
/// Returns extracted path parameters if matched.
fn match_path_pattern(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    // Handle greedy patterns (last segment is {Key+})
    let has_greedy = pattern_parts
        .last()
        .is_some_and(|p| p.starts_with('{') && p.ends_with("+}"));

    if has_greedy {
        if path_parts.len() < pattern_parts.len() {
            return None;
        }
    } else if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = Vec::new();

    for (i, (pat, actual)) in pattern_parts.iter().zip(path_parts.iter()).enumerate() {
        if pat.starts_with('{') && pat.ends_with("+}") {
            // Greedy match - capture rest of path
            let name = &pat[1..pat.len() - 2];
            let rest = path_parts[i..].join("/");
            params.push((name.to_string(), percent_decode(&rest)));
            return Some(params);
        } else if pat.starts_with('{') && pat.ends_with('}') {
            let name = &pat[1..pat.len() - 1];
            params.push((name.to_string(), percent_decode(actual)));
        } else if pat != actual {
            return None;
        }
    }

    Some(params)
}

fn parse_query_string(qs: &str) -> Vec<(String, String)> {
    if qs.is_empty() {
        return Vec::new();
    }
    qs.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let value = parts.next().unwrap_or("");
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    // Simple percent-decoding
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert x-amz-* header names to PascalCase parameter names.
/// e.g., "x-amz-copy-source" → "CopySource"
fn header_to_param_name(header: &str) -> String {
    header
        .strip_prefix("x-amz-")
        .unwrap_or(header)
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

/// Parse XML body into a JSON-like Value.
fn parse_xml_body(body: &Bytes) -> Result<Value, AwsError> {
    let s = std::str::from_utf8(body)
        .map_err(|_| AwsError::bad_request("InvalidRequest", "Body is not valid UTF-8"))?;

    parse_xml_element(s)
}

/// Simple XML → JSON parser for AWS request bodies.
fn parse_xml_element(xml: &str) -> Result<Value, AwsError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut map = serde_json::Map::new();
    let mut stack: Vec<(String, serde_json::Map<String, Value>)> = Vec::new();
    let mut current_key = String::new();
    let mut current_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if !current_key.is_empty() {
                    stack.push((current_key.clone(), map.clone()));
                    map = serde_json::Map::new();
                }
                current_key = name;
                current_text.clear();
            }
            Ok(Event::Empty(e)) => {
                // A self-closing element such as `<EventBridgeConfiguration/>`
                // carries no value but its presence is meaningful, so record
                // it as a key with an empty value rather than dropping it.
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                map.entry(name).or_insert(Value::String(String::new()));
            }
            Ok(Event::Text(e)) => {
                current_text = e.unescape().unwrap_or_default().to_string();
            }
            Ok(Event::End(_)) => {
                if current_text.is_empty() && !map.is_empty() {
                    let value = Value::Object(map.clone());
                    if let Some((parent_key, mut parent_map)) = stack.pop() {
                        // Check if this key already exists (array case)
                        if let Some(existing) = parent_map.get_mut(&current_key) {
                            match existing {
                                Value::Array(arr) => arr.push(value),
                                other => {
                                    let prev = other.take();
                                    *other = Value::Array(vec![prev, value]);
                                }
                            }
                        } else {
                            parent_map.insert(current_key.clone(), value);
                        }
                        map = parent_map;
                        current_key = parent_key;
                    } else {
                        map.insert(current_key.clone(), Value::Object(map.clone()));
                    }
                } else if !current_key.is_empty() {
                    let value = Value::String(current_text.clone());
                    if let Some((_parent_key, _parent_map)) = stack.last_mut() {}
                    map.insert(current_key.clone(), value);
                    if let Some((parent_key, mut parent_map)) = stack.pop() {
                        parent_map.insert(current_key.clone(), Value::String(current_text.clone()));
                        map = parent_map;
                        current_key = parent_key;
                    }
                }
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(AwsError::bad_request(
                    "MalformedXML",
                    format!("Invalid XML: {e}"),
                ));
            }
        }
    }

    Ok(Value::Object(map))
}

/// Serialize a restXml success response.
///
/// Special convention: if `output` contains a `__raw_body` key (base64-encoded),
/// the binary content is returned directly as the response body.  All other
/// top-level keys are placed in response headers (e.g., `Content-Type`,
/// `ETag`, `Last-Modified`).  This allows services such as S3 GetObject to
/// return arbitrary binary data.
pub fn serialize_xml_response(output: &Value, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let mut headers = HeaderMap::new();
    headers.insert("x-amz-request-id", request_id.parse().unwrap());

    // Optional `__status_code` override — used by S3 for 206 Partial Content
    // (range responses) and 304 Not Modified (conditional GETs).
    let status = output
        .get("__status_code")
        .and_then(Value::as_u64)
        .and_then(|n| StatusCode::from_u16(n as u16).ok())
        .unwrap_or(StatusCode::OK);

    // Arbitrary response headers requested by the handler via the
    // `__headers` convention (an object of header name to string value).
    // Used for responses that carry headers outside the S3 whitelist,
    // such as the `Location` on a browser POST upload redirect.
    apply_extra_headers(&mut headers, output);

    // --- Raw binary response (e.g. S3 GetObject) ---
    if let Some(raw_b64) = output.get("__raw_body").and_then(Value::as_str) {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(raw_b64)
            .unwrap_or_default();

        // Promote scalar fields to response headers.
        if let Some(map) = output.as_object() {
            for (key, val) in map {
                if key.starts_with("__") || key == "Body" {
                    continue;
                }
                let header_name = pascal_to_header(key);
                let header_value = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                if let (Ok(k), Ok(v)) = (
                    axum::http::header::HeaderName::from_bytes(header_name.as_bytes()),
                    axum::http::HeaderValue::from_str(&header_value),
                ) {
                    headers.insert(k, v);
                }
            }
        }

        // 304 Not Modified responses must not include a body.
        let body = if status == StatusCode::NOT_MODIFIED {
            Bytes::new()
        } else {
            Bytes::from(data)
        };
        return (status, headers, body);
    }

    // --- Promote well-known fields to HTTP headers (S3 convention) ---
    if let Some(map) = output.as_object() {
        for field in HEADER_BOUND_FIELDS {
            if let Some(val) = map.get(*field) {
                let header_name = pascal_to_header(field);
                let header_value = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                if let (Ok(k), Ok(v)) = (
                    axum::http::header::HeaderName::from_bytes(header_name.as_bytes()),
                    axum::http::HeaderValue::from_str(&header_value),
                ) {
                    headers.insert(k, v);
                }
            }
        }
    }

    // --- Normal XML response ---
    // If `__xml_root` is present, wrap fields in that root element (with S3 namespace).
    let xml_root = output
        .get("__xml_root")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    // Drop every sentinel key (those prefixed with `__`) before
    // serializing. Also drop fields already promoted to headers when the
    // response has no explicit XML root: those bind to headers only, so
    // emitting them as bare elements would produce a body with multiple
    // root elements (for example PutObject's ETag plus VersionId), which is
    // not well-formed XML. When an `__xml_root` wrapper is present the
    // fields are legitimate children of that element and are kept.
    let strip_header_bound = xml_root.is_none();
    let output_for_xml = if let Some(map) = output.as_object() {
        let filtered: serde_json::Map<String, Value> = map
            .iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .filter(|(k, _)| !(strip_header_bound && HEADER_BOUND_FIELDS.contains(&k.as_str())))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Value::Object(filtered)
    } else {
        output.clone()
    };

    let body = if let Some(root) = xml_root {
        // When an explicit XML root is present, always emit a root element
        // (even if there are no child fields — e.g. empty BucketLoggingStatus).
        let fields = super::query::json_to_xml_fields(&output_for_xml);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <{root} xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\n\
             {fields}</{root}>",
        )
    } else if output_for_xml.is_null()
        || (output_for_xml.is_object() && output_for_xml.as_object().unwrap().is_empty())
    {
        String::new()
    } else {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}",
            super::query::json_to_xml_fields(&output_for_xml)
        )
    };

    if !body.is_empty() && status != StatusCode::NOT_MODIFIED {
        headers.insert("content-type", "application/xml".parse().unwrap());
    }
    let body_bytes = if status == StatusCode::NOT_MODIFIED {
        Bytes::new()
    } else {
        Bytes::from(body)
    };
    (status, headers, body_bytes)
}

/// Insert handler-requested response headers carried under the
/// `__headers` convention. The value is an object mapping lowercase
/// header names to string values; non-string values are ignored, as are
/// names or values that are not valid HTTP header tokens.
fn apply_extra_headers(headers: &mut HeaderMap, output: &Value) {
    let Some(extra) = output.get("__headers").and_then(Value::as_object) else {
        return;
    };
    for (name, value) in extra {
        let Some(value) = value.as_str() else {
            continue;
        };
        if let (Ok(k), Ok(v)) = (
            axum::http::header::HeaderName::from_bytes(name.as_bytes()),
            axum::http::HeaderValue::from_str(value),
        ) {
            headers.insert(k, v);
        }
    }
}

/// Response fields that bind to HTTP headers rather than the XML body in the
/// S3 REST protocol. They are promoted to headers on every response and, for
/// responses without an explicit `__xml_root`, excluded from the body so a
/// header-only response such as PutObject serializes an empty, well-formed
/// body instead of several bare root elements.
const HEADER_BOUND_FIELDS: &[&str] = &[
    "ETag",
    "ContentType",
    "ContentLength",
    "LastModified",
    "VersionId",
    "ServerSideEncryption",
    "StorageClass",
    "DeleteMarker",
    "CopySourceVersionId",
    "SSEKMSKeyId",
    "SSECustomerAlgorithm",
    "SSECustomerKeyMD5",
];

/// Convert a PascalCase field name to a lowercase HTTP header name.
/// e.g., "ContentType" → "content-type", "ETag" → "etag"
fn pascal_to_header(name: &str) -> String {
    // Special cases where the generic PascalCase→kebab-case doesn't match HTTP conventions
    match name {
        "ETag" => return "etag".to_string(),
        "ContentType" => return "content-type".to_string(),
        "ContentLength" => return "content-length".to_string(),
        "LastModified" => return "last-modified".to_string(),
        "VersionId" => return "x-amz-version-id".to_string(),
        "ServerSideEncryption" => return "x-amz-server-side-encryption".to_string(),
        "StorageClass" => return "x-amz-storage-class".to_string(),
        "DeleteMarker" => return "x-amz-delete-marker".to_string(),
        "CopySourceVersionId" => return "x-amz-copy-source-version-id".to_string(),
        "SSEKMSKeyId" => return "x-amz-server-side-encryption-aws-kms-key-id".to_string(),
        "SSECustomerAlgorithm" => {
            return "x-amz-server-side-encryption-customer-algorithm".to_string();
        }
        "SSECustomerKeyMD5" => {
            return "x-amz-server-side-encryption-customer-key-MD5".to_string();
        }
        _ => {}
    }
    let mut out = String::new();
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// Serialize a REST-XML error response (S3, CloudFront).
///
/// The envelope is a bare `<Error>` element with `<Code>`, `<Message>`,
/// `<Resource>`, `<RequestId>`, and `<HostId>` fields, with no
/// `<ErrorResponse>` wrapper. This matches S3's wire format; the Query
/// protocol's `<ErrorResponse>` envelope is wrong for S3 and confuses
/// SDK error parsers.
///
/// Additional extras (e.g. S3's `ActualObjectSize` on 416) are emitted
/// as sibling elements inside `<Error>`. `DeleteMarker` and `VersionId`
/// are promoted to `x-amz-delete-marker` and `x-amz-version-id`
/// response headers instead.
pub fn serialize_error(error: &AwsError, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let host_id = derive_host_id(request_id);

    let resource_xml = error
        .extras
        .as_deref()
        .and_then(|extras| extras.get("Resource"))
        .and_then(Value::as_str)
        .map(|s| format!("<Resource>{}</Resource>\n", escape_xml(s)))
        .unwrap_or_default();

    let extras_xml = error
        .extras
        .as_deref()
        .map(|extras| {
            let mut buf = String::new();
            for (key, val) in extras.iter() {
                // Skip extras that are promoted to headers or the
                // dedicated <Resource> element above.
                if matches!(key.as_str(), "DeleteMarker" | "VersionId" | "Resource") {
                    continue;
                }
                let s = match val {
                    Value::String(s) => escape_xml(s),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                buf.push_str(&format!("<{key}>{s}</{key}>\n"));
            }
            buf
        })
        .unwrap_or_default();

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Error>\n\
         <Code>{code}</Code>\n\
         <Message>{message}</Message>\n\
         {resource_xml}<RequestId>{request_id}</RequestId>\n\
         <HostId>{host_id}</HostId>\n\
         {extras_xml}</Error>",
        code = escape_xml(&error.code),
        message = escape_xml(&error.message),
    );

    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/xml".parse().unwrap());
    headers.insert("x-amz-request-id", request_id.parse().unwrap());
    if let Ok(v) = host_id.parse() {
        headers.insert("x-amz-id-2", v);
    }

    if let Some(extras) = &error.extras {
        if let Some(dm) = extras.get("DeleteMarker").and_then(Value::as_bool) {
            headers.insert("x-amz-delete-marker", dm.to_string().parse().unwrap());
        }
        if let Some(vid) = extras.get("VersionId").and_then(Value::as_str)
            && let Ok(v) = vid.parse()
        {
            headers.insert("x-amz-version-id", v);
        }
    }

    (error.status, headers, Bytes::from(xml))
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Derive a deterministic opaque host id from the request id.
///
/// Real S3 emits a 76-character base64 string in `x-amz-id-2` for
/// support diagnostics. Clients treat it as opaque; we use a stable
/// SHA-256 of the request id so the same request id always yields the
/// same host id, which keeps integration tests reproducible.
fn derive_host_id(request_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request_id.as_bytes());
    let digest = hasher.finalize();
    let mut input = [0u8; 57];
    input[..32].copy_from_slice(&digest);
    input[32..].copy_from_slice(&digest[..25]);
    base64::engine::general_purpose::STANDARD.encode(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_simple_path() {
        let result = match_path_pattern("/functions", "/functions");
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_match_path_with_param() {
        let result = match_path_pattern(
            "/2015-03-31/functions/{FunctionName}",
            "/2015-03-31/functions/my-func",
        );
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(
            params[0],
            ("FunctionName".to_string(), "my-func".to_string())
        );
    }

    #[test]
    fn test_match_path_no_match() {
        let result = match_path_pattern("/functions/{Name}", "/queues/my-queue");
        assert!(result.is_none());
    }

    #[test]
    fn test_match_greedy_path() {
        let result = match_path_pattern("/{Bucket}/{Key+}", "/my-bucket/path/to/file.txt");
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].1, "my-bucket");
        assert_eq!(params[1].1, "path/to/file.txt");
    }

    #[test]
    fn test_route_matching() {
        let routes = vec![
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions",
                operation: "ListFunctions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2015-03-31/functions",
                operation: "CreateFunction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2015-03-31/functions/{FunctionName}",
                operation: "GetFunction",
                required_query_param: None,
            },
        ];

        let (op, _) = match_route("GET", "/2015-03-31/functions", "", &routes).unwrap();
        assert_eq!(op, "ListFunctions");

        let (op, params) =
            match_route("GET", "/2015-03-31/functions/my-func", "", &routes).unwrap();
        assert_eq!(op, "GetFunction");
        assert_eq!(params[0].1, "my-func");
    }

    #[test]
    fn test_query_param_disambiguation() {
        let routes = vec![
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "CreateBucket",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketVersioning",
                required_query_param: Some("versioning"),
            },
        ];

        let (op, _) = match_route("PUT", "/my-bucket", "", &routes).unwrap();
        assert_eq!(op, "CreateBucket");

        let (op, _) = match_route("PUT", "/my-bucket", "versioning", &routes).unwrap();
        assert_eq!(op, "PutBucketVersioning");
    }

    #[test]
    fn test_header_to_param_name() {
        assert_eq!(header_to_param_name("x-amz-copy-source"), "CopySource");
        assert_eq!(
            header_to_param_name("x-amz-server-side-encryption"),
            "ServerSideEncryption"
        );
    }

    #[test]
    fn serialize_error_emits_bare_error_envelope_with_host_id() {
        let err = AwsError::not_found("NoSuchBucket", "The specified bucket does not exist");
        let (status, headers, body) = serialize_error(&err, "req-1234");
        assert_eq!(status, StatusCode::NOT_FOUND);

        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<Error>"));
        assert!(!xml.contains("<ErrorResponse>"));
        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
        assert!(xml.contains("<Message>The specified bucket does not exist</Message>"));
        assert!(xml.contains("<RequestId>req-1234</RequestId>"));
        assert!(xml.contains("<HostId>"));

        assert_eq!(
            headers
                .get("x-amz-request-id")
                .and_then(|v| v.to_str().ok()),
            Some("req-1234")
        );
        assert!(headers.contains_key("x-amz-id-2"));
        assert_eq!(
            headers.get("content-type").and_then(|v| v.to_str().ok()),
            Some("application/xml")
        );
    }

    #[test]
    fn serialize_error_promotes_delete_marker_and_version_id_to_headers() {
        let err = AwsError::not_found("NoSuchKey", "The specified key does not exist")
            .with_extra("DeleteMarker", Value::Bool(true))
            .with_extra("VersionId", Value::String("abc123".to_string()));
        let (_, headers, body) = serialize_error(&err, "req-1");

        assert_eq!(
            headers
                .get("x-amz-delete-marker")
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );
        assert_eq!(
            headers
                .get("x-amz-version-id")
                .and_then(|v| v.to_str().ok()),
            Some("abc123")
        );

        let xml = std::str::from_utf8(&body).unwrap();
        assert!(!xml.contains("<DeleteMarker>"));
        assert!(!xml.contains("<VersionId>"));
    }

    #[test]
    fn serialize_error_emits_resource_and_extra_fields_as_xml() {
        let err = AwsError::range_not_satisfiable(
            "InvalidRange",
            "The requested range is not satisfiable",
        )
        .with_extra("Resource", Value::String("/bucket/key".to_string()))
        .with_extra(
            "ActualObjectSize",
            Value::Number(serde_json::Number::from(1024u64)),
        );

        let (status, _, body) = serialize_error(&err, "req-9");
        assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);

        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains("<Resource>/bucket/key</Resource>"));
        assert!(xml.contains("<ActualObjectSize>1024</ActualObjectSize>"));
    }

    #[test]
    fn serialize_error_escapes_xml_special_characters_in_message() {
        let err = AwsError::bad_request(
            "InvalidRequest",
            "Path </etc/passwd> contains <bad> & \"chars\"",
        );
        let (_, _, body) = serialize_error(&err, "req-x");
        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains("&lt;/etc/passwd&gt;"));
        assert!(xml.contains("&lt;bad&gt;"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&quot;chars&quot;"));
        // Ensure the envelope syntax isn't itself escaped.
        assert!(xml.contains("<Error>"));
        assert!(xml.contains("</Error>"));
    }

    #[test]
    fn derive_host_id_is_deterministic_and_long() {
        let a = derive_host_id("req-1");
        let b = derive_host_id("req-1");
        let c = derive_host_id("req-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Real S3 host ids are 76 chars (base64 of 57 bytes).
        assert_eq!(a.len(), 76);
    }

    #[test]
    fn parse_xml_records_self_closing_element() {
        // A self-closing element such as the aws-cli's
        // <EventBridgeConfiguration/> must survive parsing as a present key.
        let xml = "<NotificationConfiguration>\
                   <QueueConfiguration><Queue>arn:q</Queue></QueueConfiguration>\
                   <EventBridgeConfiguration/></NotificationConfiguration>";
        let value = parse_xml_element(xml).expect("parse");
        let nested = value
            .get("NotificationConfiguration")
            .and_then(|n| n.get("EventBridgeConfiguration"));
        let top = value.get("EventBridgeConfiguration");
        assert!(
            nested.is_some() || top.is_some(),
            "self-closing element should be present: {value:?}"
        );
    }

    #[test]
    fn header_only_response_has_empty_body() {
        // A versioned PutObject returns ETag and VersionId, both header-bound.
        // Without an XML root they must not become bare body elements, which
        // would be multiple roots and therefore malformed XML.
        let output = serde_json::json!({
            "ETag": "\"abc\"",
            "VersionId": "v1",
        });
        let (status, headers, body) = serialize_xml_response(&output, "req-1");
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_empty(), "body should be empty, got {body:?}");
        assert_eq!(headers.get("etag").unwrap(), "\"abc\"");
        assert_eq!(headers.get("x-amz-version-id").unwrap(), "v1");
    }

    #[test]
    fn delete_marker_response_has_empty_body() {
        // delete_object reports DeleteMarker as a JSON bool, so the
        // promotion path must render it as the "true" header value.
        let output = serde_json::json!({
            "DeleteMarker": true,
            "VersionId": "v2",
        });
        let (_, headers, body) = serialize_xml_response(&output, "req-2");
        assert!(body.is_empty(), "body should be empty, got {body:?}");
        assert_eq!(headers.get("x-amz-delete-marker").unwrap(), "true");
        assert_eq!(headers.get("x-amz-version-id").unwrap(), "v2");
    }

    #[test]
    fn xml_root_response_keeps_header_bound_fields_in_body() {
        // CompleteMultipartUpload wraps its fields, including ETag, in a root
        // element, so the ETag must stay in the body.
        let output = serde_json::json!({
            "__xml_root": "CompleteMultipartUploadResult",
            "ETag": "\"xyz\"",
            "Key": "obj",
        });
        let (_, _, body) = serialize_xml_response(&output, "req-3");
        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains("<CompleteMultipartUploadResult"));
        assert!(xml.contains("<ETag>\"xyz\"</ETag>"), "xml: {xml}");
        assert!(xml.contains("<Key>obj</Key>"));
    }
}
