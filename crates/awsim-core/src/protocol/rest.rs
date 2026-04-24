use axum::http::{HeaderMap, Method, StatusCode, Uri};
use bytes::Bytes;
use serde_json::Value;

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
        // Extract relevant headers (x-amz-*)
        for (name, value) in headers.iter() {
            let name_str = name.as_str();
            if name_str.starts_with("x-amz-") && name_str != "x-amz-target"
                && let Ok(v) = value.to_str() {
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

    // --- Raw binary response (e.g. S3 GetObject) ---
    if let Some(raw_b64) = output.get("__raw_body").and_then(Value::as_str) {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(raw_b64)
            .unwrap_or_default();

        // Promote scalar fields to response headers.
        if let Some(map) = output.as_object() {
            for (key, val) in map {
                if key == "__raw_body" || key == "Body" {
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

        return (StatusCode::OK, headers, Bytes::from(data));
    }

    // --- Promote well-known fields to HTTP headers (S3 convention) ---
    if let Some(map) = output.as_object() {
        let header_fields = [
            "ETag",
            "ContentType",
            "ContentLength",
            "LastModified",
            "VersionId",
            "ServerSideEncryption",
            "StorageClass",
        ];
        for field in &header_fields {
            if let Some(val) = map.get(*field) {
                let header_name = pascal_to_header(field);
                let header_value = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
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

    let output_for_xml = if xml_root.is_some() {
        // Build a Value without the __xml_root sentinel key.
        if let Some(map) = output.as_object() {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| k.as_str() != "__xml_root")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(filtered)
        } else {
            output.clone()
        }
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

    if !body.is_empty() {
        headers.insert("content-type", "application/xml".parse().unwrap());
    }
    (StatusCode::OK, headers, Bytes::from(body))
}

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
}
