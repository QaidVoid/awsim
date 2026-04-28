//! API Gateway proxy handler for the main `awsim` binary.
//!
//! Handles requests at `/restapis/{api_id}/{stage}/{*path}` and routes them
//! to the appropriate Lambda function via the API Gateway service state.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use axum::response::IntoResponse;
use bytes::Bytes;
use serde_json::json;
use tracing::{debug, warn};

use awsim_apigateway::{ApiGatewayService, ApiGatewayV1Service};
use awsim_core::{RequestContext, ServiceHandler};

/// State provided to the proxy handler.
#[derive(Clone)]
pub struct ProxyState {
    /// The API Gateway v2 (HTTP APIs) service.
    pub apigw: Arc<ApiGatewayService>,
    /// The API Gateway v1 (REST APIs) service.
    pub apigw_v1: Arc<ApiGatewayV1Service>,
    /// The Lambda service (used to invoke functions).
    pub lambda: Option<Arc<dyn ServiceHandler>>,
    /// Outbound HTTP client used by HTTP / HTTP_PROXY integrations.
    /// Built once and shared so the connection pool is reused.
    pub http_client: reqwest::Client,
    pub default_account_id: String,
    pub default_region: String,
}

/// Axum handler for `/restapis/{api_id}/{stage}/_user_request_/{*path}`
/// and the bare `/restapis/{api_id}/{stage}/_user_request_` (no path).
///
/// The literal `_user_request_` segment is required so management routes
/// like `/restapis/{id}/resources/...` and `/restapis/{id}/authorizers`
/// don't get accidentally swallowed by this proxy.
pub async fn handle_proxy(
    State(state): State<ProxyState>,
    Path(params): Path<Vec<(String, String)>>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> impl IntoResponse {
    let api_id = params
        .iter()
        .find(|(k, _)| k == "api_id")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let stage = params
        .iter()
        .find(|(k, _)| k == "stage")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let path = params
        .iter()
        .find(|(k, _)| k == "path")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    debug!(
        api_id = %api_id,
        stage = %stage,
        path = %path,
        method = %method,
        "API Gateway proxy request"
    );

    // Get the API Gateway state for the default account/region.
    let agw_state = state
        .apigw
        .store()
        .get(&state.default_account_id, &state.default_region);

    let path_with_slash = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}")
    };
    let query_string = uri.query().unwrap_or("");

    // Route the request through the API Gateway proxy logic.
    let proxy_result = awsim_apigateway::proxy_request(
        &api_id,
        &stage,
        method.as_str(),
        &path_with_slash,
        query_string,
        &headers,
        &body,
        &agw_state,
    )
    .await;

    if let Some(proxy) = proxy_result {
        return invoke_lambda(&state, &method, &uri, &proxy.integration_uri, proxy.event).await;
    }

    // No v2 match — try v1 (REST APIs). Same id namespace from the
    // caller's perspective; we check both stores in turn.
    let v1_state = state
        .apigw_v1
        .store()
        .get(&state.default_account_id, &state.default_region);
    let headers_map: std::collections::HashMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_string(), v.to_string()))
        })
        .collect();
    let v1_match = awsim_apigateway::v1::proxy_request(
        &v1_state,
        &api_id,
        &stage,
        method.as_str(),
        &path_with_slash,
        query_string,
        &headers_map,
        &body,
    );

    match v1_match {
        Some(m) => dispatch_v1(&state, &method, &uri, &headers, &body, query_string, m).await,
        None => {
            warn!(
                api_id = %api_id,
                path = %path_with_slash,
                method = %method,
                "No matching route found in API Gateway (tried both v1 and v2)"
            );
            error_response(
                StatusCode::NOT_FOUND,
                &format!("No route found for {method} {path_with_slash} in API {api_id}"),
            )
        }
    }
}

/// Dispatch a matched v1 (REST APIs) integration based on its type. MOCK,
/// AWS/AWS_PROXY (Lambda) and HTTP/HTTP_PROXY (outbound fetch) are all
/// handled; anything else returns 501.
#[allow(clippy::too_many_arguments)]
async fn dispatch_v1(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    query_string: &str,
    m: awsim_apigateway::V1ProxyMatch,
) -> Response<Body> {
    debug!(
        integration_type = %m.integration_type,
        resource = %m.matched_resource_path,
        "v1 stage invocation matched"
    );
    match m.integration_type.as_str() {
        "MOCK" => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
        "AWS" | "AWS_PROXY" => invoke_lambda(state, method, uri, &m.integration_uri, m.event).await,
        // HTTP and HTTP_PROXY both forward the request upstream. Real AWS
        // also runs request/response mapping templates for HTTP (the
        // non-proxy variant), but most users configure HTTP_PROXY anyway —
        // and an unmapped pass-through is strictly more useful than 501.
        "HTTP" | "HTTP_PROXY" => {
            proxy_http(
                &state.http_client,
                method,
                &m.integration_uri,
                headers,
                body,
                query_string,
            )
            .await
        }
        other => error_response(
            StatusCode::NOT_IMPLEMENTED,
            &format!("Integration type {other} is not yet supported by AWSim"),
        ),
    }
}

/// Forward an incoming request to the integration URI with reqwest and
/// stream the upstream response back unchanged. Drops `host` / `content-length`
/// since reqwest sets them itself.
async fn proxy_http(
    client: &reqwest::Client,
    method: &Method,
    integration_uri: &str,
    headers: &HeaderMap,
    body: &Bytes,
    query_string: &str,
) -> Response<Body> {
    let target_url = if query_string.is_empty() {
        integration_uri.to_string()
    } else if integration_uri.contains('?') {
        format!("{integration_uri}&{query_string}")
    } else {
        format!("{integration_uri}?{query_string}")
    };

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Invalid method for upstream: {e}"),
            );
        }
    };

    let mut req = client.request(reqwest_method, &target_url);
    for (name, value) in headers.iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if lname == "host" || lname == "content-length" {
            continue;
        }
        if let Ok(v) = value.to_str() {
            req = req.header(name.as_str(), v);
        }
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(target = %target_url, error = %e, "HTTP proxy upstream request failed");
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Upstream request failed: {e}"),
            );
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (name, value) in upstream.headers().iter() {
        // Skip framing headers; axum / hyper will set them based on the
        // forwarded body. Keeping them around can trigger duplicate or
        // mismatched values when the body is re-encoded.
        let lname = name.as_str().to_ascii_lowercase();
        if lname == "transfer-encoding" || lname == "content-length" || lname == "connection" {
            continue;
        }
        if let Ok(header_value) = axum::http::HeaderValue::from_bytes(value.as_bytes()) {
            builder = builder.header(name.as_str(), header_value);
        }
    }
    let bytes = match upstream.bytes().await {
        Ok(b) => b,
        Err(e) => {
            warn!(target = %target_url, error = %e, "Failed to read upstream body");
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read upstream body: {e}"),
            );
        }
    };
    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap()
    })
}

async fn invoke_lambda(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    integration_uri: &str,
    event: serde_json::Value,
) -> Response<Body> {
    let lambda_handler = match &state.lambda {
        Some(h) => Arc::clone(h),
        None => {
            warn!("Lambda service not registered — cannot invoke function");
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Lambda service not registered",
            );
        }
    };

    let function_name = extract_function_name(integration_uri);
    let ctx = RequestContext {
        account_id: state.default_account_id.clone(),
        region: state.default_region.clone(),
        service: "lambda".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: method.to_string(),
        uri: uri.to_string(),
        event_bus: None,
    };
    let invoke_input = json!({
        "FunctionName": function_name,
        "InvocationType": "RequestResponse",
        "Payload": event,
    });

    match lambda_handler.handle("Invoke", invoke_input, &ctx).await {
        Ok(result) => lambda_response_to_http(result),
        Err(e) => {
            warn!(
                function = %function_name,
                error = %e.message,
                "Lambda invocation failed"
            );
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Lambda invocation error: {}", e.message),
            )
        }
    }
}

/// Convert a Lambda Invoke result into an HTTP response.
///
/// Lambda v2 payload format response:
/// ```json
/// {
///   "statusCode": 200,
///   "headers": { "content-type": "application/json" },
///   "body": "{...}",
///   "isBase64Encoded": false
/// }
/// ```
fn lambda_response_to_http(result: serde_json::Value) -> Response<Body> {
    // If the result has a "statusCode" field, treat it as a proxy response.
    if let Some(status_code) = result.get("statusCode").and_then(|v| v.as_u64()) {
        let status =
            StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let mut builder = Response::builder().status(status);

        // Apply response headers from Lambda.
        if let Some(resp_headers) = result.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in resp_headers {
                if let Some(v) = value.as_str()
                    && let Ok(header_value) = v.parse::<axum::http::HeaderValue>()
                {
                    builder = builder.header(key.as_str(), header_value);
                }
            }
        }

        let is_base64 = result
            .get("isBase64Encoded")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let body_bytes: Bytes = match result.get("body") {
            None => Bytes::new(),
            Some(serde_json::Value::Null) => Bytes::new(),
            Some(serde_json::Value::String(s)) => {
                if is_base64 {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(s)
                        .unwrap_or_else(|_| s.as_bytes().to_vec())
                        .into()
                } else {
                    Bytes::from(s.clone())
                }
            }
            Some(other) => Bytes::from(other.to_string()),
        };

        builder.body(Body::from(body_bytes)).unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
    } else {
        // Raw result — serialize as JSON.
        let body = serde_json::to_vec(&result).unwrap_or_default();
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    }
}

/// Extract a Lambda function name from an ARN or plain name.
///
/// Handles:
/// - Full ARN: `arn:aws:lambda:us-east-1:000000000000:function:my-func`
/// - Plain name: `my-func`
fn extract_function_name(uri: &str) -> String {
    if uri.starts_with("arn:aws:lambda:") {
        // ARN format: arn:aws:lambda:{region}:{account}:function:{name}
        uri.split(':').next_back().unwrap_or(uri).to_string()
    } else {
        uri.to_string()
    }
}

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({
        "message": message,
    }))
    .unwrap_or_default();

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}
