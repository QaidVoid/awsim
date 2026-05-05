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

use awsim_apigateway::vtl::{self, RenderContext};
use awsim_apigateway::{
    ApiGatewayService, ApiGatewayV1Service, AuthorizationOutcome, AuthorizationStep, Integration,
    IntegrationResponse, LambdaInvocation, V1ProxyMatch, apply_lambda_response,
};
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
        &state.default_account_id,
        &state.default_region,
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
    mut m: V1ProxyMatch,
) -> Response<Body> {
    debug!(
        integration_type = %m.integration_type,
        resource = %m.matched_resource_path,
        "v1 stage invocation matched"
    );

    // Drive the authorizer state machine to a terminal step. Custom
    // Lambda authorizers may take one Lambda call before resolving.
    let outcome = match resolve_authorization(state, method, uri, &mut m).await {
        AuthResolution::Allowed(o) => Some(o),
        AuthResolution::None => None,
        AuthResolution::Unauthorized(reason) => {
            return error_response(StatusCode::UNAUTHORIZED, &reason);
        }
        AuthResolution::Forbidden(reason) => {
            return error_response(StatusCode::FORBIDDEN, &reason);
        }
    };
    if let Some(outcome) = outcome {
        merge_authorizer_into_event(&mut m, &outcome);
    }

    let render_ctx = render_context_from_match(&m, body);
    match m.integration_type.as_str() {
        "MOCK" => dispatch_mock(&m, &render_ctx),
        "AWS_PROXY" => invoke_lambda(state, method, uri, &m.integration_uri, m.event).await,
        "AWS" => dispatch_aws_non_proxy(state, method, uri, &m, &render_ctx).await,
        // HTTP_PROXY forwards verbatim — no template processing, no
        // method/integration response mapping.
        "HTTP_PROXY" => {
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
        // HTTP applies request templates before forwarding and response
        // templates on the way back.
        "HTTP" => {
            dispatch_http_non_proxy(
                &state.http_client,
                method,
                headers,
                query_string,
                &m,
                &render_ctx,
            )
            .await
        }
        other => error_response(
            StatusCode::NOT_IMPLEMENTED,
            &format!("Integration type {other} is not yet supported by AWSim"),
        ),
    }
}

enum AuthResolution {
    None,
    Allowed(AuthorizationOutcome),
    Unauthorized(String),
    Forbidden(String),
}

/// Drive the authorizer state machine. Most cases resolve in one step;
/// custom Lambda authorizers take one Lambda invocation first, then
/// `apply_lambda_response` returns the final step. The loop bound is
/// 2 — there's no scenario in which a single authorizer needs more
/// than one Lambda round-trip.
async fn resolve_authorization(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    m: &mut V1ProxyMatch,
) -> AuthResolution {
    // Take ownership so we can re-assign as the state machine runs.
    let mut step = std::mem::replace(&mut m.authorization, AuthorizationStep::NotConfigured);
    for _ in 0..2 {
        match step {
            AuthorizationStep::NotConfigured => return AuthResolution::None,
            AuthorizationStep::Allowed(outcome) => return AuthResolution::Allowed(outcome),
            AuthorizationStep::Unauthorized(reason) => return AuthResolution::Unauthorized(reason),
            AuthorizationStep::Forbidden(reason) => return AuthResolution::Forbidden(reason),
            AuthorizationStep::InvokeLambda(invocation) => {
                let response = match invoke_authorizer_lambda(state, method, uri, &invocation).await
                {
                    Ok(v) => v,
                    Err(e) => {
                        return AuthResolution::Forbidden(format!(
                            "Lambda authorizer invocation failed: {e}"
                        ));
                    }
                };
                let cache = &state.apigw_v1.store().get(&state.default_account_id, &state.default_region).authorizer_cache;
                step = apply_lambda_response(cache, &invocation, &response);
            }
        }
    }
    AuthResolution::Forbidden(
        "Authorizer state machine did not converge after one Lambda round-trip".to_string(),
    )
}

async fn invoke_authorizer_lambda(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    invocation: &LambdaInvocation,
) -> Result<serde_json::Value, String> {
    let lambda = state
        .lambda
        .as_ref()
        .ok_or_else(|| "Lambda service not registered — cannot invoke authorizer".to_string())?;
    let function_name = extract_function_name(&invocation.authorizer_uri);
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
        "Payload": invocation.event.clone(),
    });
    let result = lambda
        .handle("Invoke", invoke_input, &ctx)
        .await
        .map_err(|e| e.message)?;
    // The Lambda service returns the function's raw return value.
    Ok(result)
}

/// Fold the authorizer's outcome into the proxy event so the integration
/// — and any VTL templates — can access it as `requestContext.authorizer`
/// or `$context.authorizer`.
fn merge_authorizer_into_event(m: &mut V1ProxyMatch, outcome: &AuthorizationOutcome) {
    let mut authorizer = serde_json::Map::new();
    authorizer.insert(
        "principalId".to_string(),
        serde_json::Value::String(outcome.principal_id.clone()),
    );
    if let Some(ctx_obj) = outcome.context.as_object() {
        for (k, v) in ctx_obj {
            authorizer.insert(k.clone(), v.clone());
        }
    }
    let authorizer_value = serde_json::Value::Object(authorizer);

    if let Some(rc) = m
        .event
        .get_mut("requestContext")
        .and_then(serde_json::Value::as_object_mut)
    {
        rc.insert("authorizer".to_string(), authorizer_value.clone());
    }
    if let Some(rc) = m.request_context.as_object_mut() {
        rc.insert("authorizer".to_string(), authorizer_value);
    }
}

fn render_context_from_match(m: &V1ProxyMatch, body: &Bytes) -> RenderContext {
    RenderContext {
        body: std::str::from_utf8(body).unwrap_or("").to_string(),
        path_params: m.path_params.clone(),
        query_params: m.query_params.clone(),
        headers: m.headers.clone(),
        stage_variables: m.stage_variables.clone(),
        request_context: m.request_context.clone(),
    }
}

/// MOCK integrations don't reach a backend. AWS resolves them by:
/// 1. Rendering the request template — its JSON output drives which
///    integration response to pick (via the `statusCode` field).
/// 2. Rendering the chosen integration response's template as the body.
fn dispatch_mock(m: &V1ProxyMatch, render_ctx: &RenderContext) -> Response<Body> {
    let request_template = pick_template(&m.integration.request_templates, render_ctx);
    let rendered_request = request_template
        .as_deref()
        .map(|t| vtl::render(t, render_ctx))
        .unwrap_or_default();
    let status_code = serde_json::from_str::<serde_json::Value>(&rendered_request)
        .ok()
        .and_then(|v| v.get("statusCode").and_then(|s| s.as_u64()))
        .unwrap_or(200);
    let status_str = status_code.to_string();

    let integration_response = m
        .integration
        .integration_responses
        .get(&status_str)
        .or_else(|| pick_default_response(&m.integration));

    let body = integration_response
        .and_then(|r| pick_template(&r.response_templates, render_ctx))
        .map(|t| vtl::render(&t, render_ctx))
        .unwrap_or_else(|| "{}".to_string());

    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::OK))
        .header("content-type", "application/json");
    if let Some(r) = integration_response {
        builder = apply_response_parameters(builder, &r.response_parameters, render_ctx);
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| empty_500())
}

async fn dispatch_aws_non_proxy(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    m: &V1ProxyMatch,
    render_ctx: &RenderContext,
) -> Response<Body> {
    let request_template = pick_template(&m.integration.request_templates, render_ctx);
    let rendered_request = request_template
        .as_deref()
        .map(|t| vtl::render(t, render_ctx))
        .unwrap_or_else(|| render_ctx.body.clone());
    let payload = serde_json::from_str::<serde_json::Value>(&rendered_request)
        .unwrap_or_else(|_| serde_json::Value::String(rendered_request.clone()));

    let result = match invoke_lambda_raw(state, method, uri, &m.integration_uri, payload).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let upstream_body = match &result {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let integration_response = pick_integration_response(&m.integration, &upstream_body);
    let status = integration_response
        .map(|r| r.status_code.parse::<u16>().unwrap_or(200))
        .unwrap_or(200);

    let body = integration_response
        .and_then(|r| pick_template(&r.response_templates, render_ctx))
        .map(|t| {
            let mut body_ctx = render_ctx.clone();
            body_ctx.body = upstream_body.clone();
            vtl::render(&t, &body_ctx)
        })
        .unwrap_or(upstream_body);

    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .header("content-type", "application/json");
    if let Some(r) = integration_response {
        builder = apply_response_parameters(builder, &r.response_parameters, render_ctx);
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| empty_500())
}

async fn dispatch_http_non_proxy(
    client: &reqwest::Client,
    method: &Method,
    headers: &HeaderMap,
    query_string: &str,
    m: &V1ProxyMatch,
    render_ctx: &RenderContext,
) -> Response<Body> {
    let request_template = pick_template(&m.integration.request_templates, render_ctx);
    let rendered_request = request_template
        .as_deref()
        .map(|t| vtl::render(t, render_ctx))
        .unwrap_or_else(|| render_ctx.body.clone());
    let mapped_body = Bytes::from(rendered_request);

    let upstream_resp = match perform_http(
        client,
        method,
        &m.integration_uri,
        headers,
        &mapped_body,
        query_string,
    )
    .await
    {
        Ok(resp) => resp,
        Err(resp) => return resp,
    };

    let upstream_body = String::from_utf8_lossy(&upstream_resp.body).into_owned();
    let integration_response = pick_integration_response(&m.integration, &upstream_body);
    let status = integration_response
        .map(|r| r.status_code.parse::<u16>().unwrap_or(upstream_resp.status))
        .unwrap_or(upstream_resp.status);

    let body = integration_response
        .and_then(|r| pick_template(&r.response_templates, render_ctx))
        .map(|t| {
            let mut body_ctx = render_ctx.clone();
            body_ctx.body = upstream_body.clone();
            vtl::render(&t, &body_ctx)
        })
        .unwrap_or(upstream_body);

    let mut builder =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
    for (name, value) in upstream_resp.headers.iter() {
        builder = builder.header(name, value);
    }
    if let Some(r) = integration_response {
        builder = apply_response_parameters(builder, &r.response_parameters, render_ctx);
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| empty_500())
}

/// Pick the request/response template whose key matches the request's
/// content-type. Falls back to `application/json`, then any single key.
fn pick_template(
    map: &std::collections::HashMap<String, String>,
    ctx: &RenderContext,
) -> Option<String> {
    if map.is_empty() {
        return None;
    }
    let content_type = ctx
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();
    if let Some(t) = map.get(&content_type) {
        return Some(t.clone());
    }
    if let Some(t) = map.get("application/json") {
        return Some(t.clone());
    }
    map.values().next().cloned()
}

fn pick_integration_response<'a>(
    integration: &'a Integration,
    upstream: &str,
) -> Option<&'a IntegrationResponse> {
    let mut default: Option<&IntegrationResponse> = None;
    for r in integration.integration_responses.values() {
        if r.selection_pattern.is_empty() {
            default = Some(r);
            continue;
        }
        if regex_like_match(&r.selection_pattern, upstream) {
            return Some(r);
        }
    }
    default.or_else(|| pick_default_response(integration))
}

fn pick_default_response(integration: &Integration) -> Option<&IntegrationResponse> {
    integration
        .integration_responses
        .values()
        .find(|r| r.selection_pattern.is_empty())
        .or_else(|| integration.integration_responses.values().next())
}

/// Approximate AWS' regex selection pattern. We don't pull in a regex
/// crate just for this — a literal substring check covers the common
/// `5\d\d` / `4\d\d` / `Error.*` patterns well enough that templates
/// at least exercise the right branch in tests.
fn regex_like_match(pattern: &str, body: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if pattern == ".*" {
        return true;
    }
    body.contains(pattern.trim_start_matches('^').trim_end_matches('$'))
}

fn apply_response_parameters(
    mut builder: axum::http::response::Builder,
    parameters: &std::collections::HashMap<String, String>,
    ctx: &RenderContext,
) -> axum::http::response::Builder {
    for (key, raw_value) in parameters {
        let header_name = match key.strip_prefix("method.response.header.") {
            Some(n) => n,
            None => continue,
        };
        let resolved = resolve_response_parameter(raw_value, ctx);
        builder = builder.header(header_name, resolved);
    }
    builder
}

/// Response parameter values come in two forms: a literal `'string'` or
/// a reference like `integration.response.body.foo` / `integration.response.header.bar`.
/// Strings without a leading single quote are treated as VTL templates,
/// which covers most user-authored values.
fn resolve_response_parameter(raw: &str, ctx: &RenderContext) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    vtl::render(trimmed, ctx)
}

struct UpstreamResponse {
    status: u16,
    headers: HeaderMap,
    body: Bytes,
}

async fn perform_http(
    client: &reqwest::Client,
    method: &Method,
    integration_uri: &str,
    headers: &HeaderMap,
    body: &Bytes,
    query_string: &str,
) -> Result<UpstreamResponse, Response<Body>> {
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
            return Err(error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Invalid method for upstream: {e}"),
            ));
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
    let upstream = req.send().await.map_err(|e| {
        warn!(target = %target_url, error = %e, "HTTP integration upstream request failed");
        error_response(
            StatusCode::BAD_GATEWAY,
            &format!("Upstream request failed: {e}"),
        )
    })?;

    let status = upstream.status().as_u16();
    let mut hdrs = HeaderMap::new();
    for (name, value) in upstream.headers() {
        let lname = name.as_str().to_ascii_lowercase();
        if lname == "transfer-encoding" || lname == "content-length" || lname == "connection" {
            continue;
        }
        if let Ok(v) = axum::http::HeaderValue::from_bytes(value.as_bytes()) {
            hdrs.insert(name.clone(), v);
        }
    }
    let body = upstream.bytes().await.map_err(|e| {
        warn!(target = %target_url, error = %e, "Failed to read upstream body");
        error_response(
            StatusCode::BAD_GATEWAY,
            &format!("Failed to read upstream body: {e}"),
        )
    })?;
    Ok(UpstreamResponse {
        status,
        headers: hdrs,
        body,
    })
}

fn empty_500() -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::empty())
        .unwrap()
}

async fn invoke_lambda_raw(
    state: &ProxyState,
    method: &Method,
    uri: &Uri,
    integration_uri: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, Response<Body>> {
    let lambda_handler = match &state.lambda {
        Some(h) => Arc::clone(h),
        None => {
            warn!("Lambda service not registered — cannot invoke function");
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Lambda service not registered",
            ));
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
        "Payload": payload,
    });
    lambda_handler
        .handle("Invoke", invoke_input, &ctx)
        .await
        .map_err(|e| {
            warn!(
                function = %function_name,
                error = %e.message,
                "Lambda invocation failed"
            );
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Lambda invocation error: {}", e.message),
            )
        })
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
