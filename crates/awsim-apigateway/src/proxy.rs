//! API Gateway proxy routing — matches incoming HTTP requests to Lambda integrations.
//!
//! When a request arrives at `/restapis/{api_id}/{stage}/{*path}`, this module:
//! 1. Looks up the API by api_id.
//! 2. Matches the HTTP method + path against the API's routes.
//! 3. Finds the Lambda integration for the matched route.
//! 4. Builds an API Gateway v2 proxy event.
//! 5. Returns the event payload (actual Lambda invocation is delegated to the Lambda service).

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::HeaderMap;
use serde_json::{Value, json};

use crate::state::ApiGatewayState;
use crate::util::{epoch_to_clf, now_epoch};

/// Result of a successful proxy route match.
pub struct ProxyResponse {
    /// The Lambda function ARN to invoke.
    pub integration_uri: String,
    /// The payload format version (e.g., "2.0").
    pub payload_format_version: String,
    /// The assembled API Gateway v2 event.
    pub event: Value,
    /// The matched route key (e.g., "GET /items").
    pub route_key: String,
}

/// Attempt to route an incoming HTTP request to a Lambda integration.
///
/// Returns `None` if no matching API or route is found.
pub async fn proxy_request(
    api_id: &str,
    stage: &str,
    method: &str,
    path: &str,
    query_string: &str,
    headers: &HeaderMap,
    body: &[u8],
    state: &Arc<ApiGatewayState>,
) -> Option<ProxyResponse> {
    let api = state.apis.get(api_id)?;

    // Match route: try specific routes first, then fall back to $default.
    let (matched_route_key, integration_id) = match_route(&api.routes, method, path)?;

    let integration = api.integrations.get(&integration_id)?;
    let integration_uri = integration.integration_uri.clone();
    let payload_format_version = integration.payload_format_version.clone();

    // Build headers map for the event.
    let headers_map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| {
            let k = name.as_str().to_string();
            let v = value.to_str().ok()?.to_string();
            Some((k, v))
        })
        .collect();

    let epoch = now_epoch();
    let time_str = epoch_to_clf(epoch);

    // Build API Gateway v2 payload format event.
    let event = if payload_format_version == "2.0" {
        let body_value = if body.is_empty() {
            Value::Null
        } else {
            match std::str::from_utf8(body) {
                Ok(s) => Value::String(s.to_string()),
                Err(_) => {
                    use base64::Engine;
                    Value::String(
                        base64::engine::general_purpose::STANDARD.encode(body),
                    )
                }
            }
        };
        let is_base64 = !body.is_empty() && std::str::from_utf8(body).is_err();

        json!({
            "version": "2.0",
            "routeKey": matched_route_key,
            "rawPath": path,
            "rawQueryString": query_string,
            "headers": headers_map,
            "requestContext": {
                "apiId": api_id,
                "http": {
                    "method": method,
                    "path": path,
                    "protocol": "HTTP/1.1",
                    "sourceIp": "127.0.0.1",
                    "userAgent": headers_map.get("user-agent").cloned().unwrap_or_default(),
                },
                "stage": stage,
                "time": time_str,
                "timeEpoch": epoch,
                "requestId": uuid::Uuid::new_v4().to_string(),
                "accountId": "000000000000",
            },
            "body": body_value,
            "isBase64Encoded": is_base64,
        })
    } else {
        // Payload format 1.0 (legacy REST API style)
        let body_str = std::str::from_utf8(body).ok().map(|s| s.to_string());
        json!({
            "version": "1.0",
            "resource": path,
            "path": path,
            "httpMethod": method,
            "headers": headers_map,
            "queryStringParameters": parse_query_params(query_string),
            "requestContext": {
                "apiId": api_id,
                "httpMethod": method,
                "path": path,
                "stage": stage,
                "requestTime": time_str,
                "requestTimeEpoch": epoch,
            },
            "body": body_str,
            "isBase64Encoded": false,
        })
    };

    Some(ProxyResponse {
        integration_uri,
        payload_format_version,
        event,
        route_key: matched_route_key,
    })
}

/// Match HTTP method + path against stored routes.
///
/// Returns `(route_key, integration_id)` on success.
/// Falls back to `$default` if no specific route matches.
fn match_route(
    routes: &HashMap<String, crate::state::ApiRoute>,
    method: &str,
    path: &str,
) -> Option<(String, String)> {
    let mut default_route: Option<(String, String)> = None;

    for route in routes.values() {
        if route.route_key == "$default" {
            if let Some(ref target) = route.target {
                let integration_id = extract_integration_id(target)?;
                default_route = Some(("$default".to_string(), integration_id));
            }
            continue;
        }

        // Route key format: "METHOD /path" or "ANY /path"
        let (route_method, route_path) = parse_route_key(&route.route_key)?;

        let method_matches = route_method == "ANY"
            || route_method.eq_ignore_ascii_case(method);

        if method_matches && path_matches(route_path, path) {
            if let Some(ref target) = route.target {
                let integration_id = extract_integration_id(target)?;
                return Some((route.route_key.clone(), integration_id));
            }
        }
    }

    default_route
}

/// Parse a route key like "GET /items/{id}" into ("GET", "/items/{id}").
fn parse_route_key(route_key: &str) -> Option<(&str, &str)> {
    let mut parts = route_key.splitn(2, ' ');
    let method = parts.next()?;
    let path = parts.next()?;
    Some((method, path))
}

/// Extract integration ID from a target string like "integrations/abc123".
fn extract_integration_id(target: &str) -> Option<String> {
    target
        .strip_prefix("integrations/")
        .map(|s| s.to_string())
        .or_else(|| Some(target.to_string()))
}

/// Simple path pattern matching supporting `{param}` placeholders.
/// Does NOT support greedy `{param+}` — add if needed.
fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return false;
    }

    for (pat, actual) in pattern_parts.iter().zip(path_parts.iter()) {
        if pat.starts_with('{') && pat.ends_with('}') {
            // Path parameter — always matches
            continue;
        }
        if !pat.eq_ignore_ascii_case(actual) {
            return false;
        }
    }

    true
}

fn parse_query_params(qs: &str) -> HashMap<String, String> {
    if qs.is_empty() {
        return HashMap::new();
    }
    qs.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.to_string();
            let value = parts.next().unwrap_or("").to_string();
            Some((key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches_exact() {
        assert!(path_matches("/items", "/items"));
        assert!(!path_matches("/items", "/orders"));
    }

    #[test]
    fn test_path_matches_param() {
        assert!(path_matches("/items/{id}", "/items/42"));
        assert!(!path_matches("/items/{id}", "/items/42/extra"));
    }

    #[test]
    fn test_parse_route_key() {
        let (method, path) = parse_route_key("GET /items/{id}").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/items/{id}");
    }

    #[test]
    fn test_extract_integration_id() {
        assert_eq!(
            extract_integration_id("integrations/abc123").unwrap(),
            "abc123"
        );
    }
}
