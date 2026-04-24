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

use awsim_apigateway::ApiGatewayService;
use awsim_core::{RequestContext, ServiceHandler};

/// State provided to the proxy handler.
#[derive(Clone)]
pub struct ProxyState {
    /// The API Gateway service (used to look up routes and integrations).
    pub apigw: Arc<ApiGatewayService>,
    /// The Lambda service (used to invoke functions).
    pub lambda: Option<Arc<dyn ServiceHandler>>,
    pub default_account_id: String,
    pub default_region: String,
}

/// Axum handler for `/restapis/{api_id}/{stage}/{*path}`.
pub async fn handle_proxy(
    State(state): State<ProxyState>,
    Path((api_id, stage, path)): Path<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> impl IntoResponse {
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

    let path_with_slash = format!("/{path}");
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

    match proxy_result {
        None => {
            warn!(
                api_id = %api_id,
                path = %path_with_slash,
                method = %method,
                "No matching route found in API Gateway"
            );
            error_response(
                StatusCode::NOT_FOUND,
                &format!("No route found for {method} {path_with_slash} in API {api_id}"),
            )
        }
        Some(proxy) => {
            // Find the Lambda handler.
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

            // Extract the function name from the integration URI (Lambda ARN or plain name).
            let function_name = extract_function_name(&proxy.integration_uri);

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

            // Build the Lambda Invoke input.
            let invoke_input = json!({
                "FunctionName": function_name,
                "InvocationType": "RequestResponse",
                "Payload": proxy.event,
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
                    && let Ok(header_value) = v.parse::<axum::http::HeaderValue>() {
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
