use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use bytes::Bytes;
use tracing::{debug, info, warn};

use crate::auth;
use crate::error::AwsError;
use crate::events::EventBus;
use crate::protocol::{self, Protocol, RouteDefinition};
use crate::ServiceHandler;

/// Shared application state passed to all request handlers.
#[derive(Clone)]
pub struct AppState {
    /// Registered service handlers, keyed by signing name.
    pub services: Arc<HashMap<String, Arc<dyn ServiceHandler>>>,
    /// Route definitions for REST-protocol services, keyed by signing name.
    pub routes: Arc<HashMap<String, Vec<RouteDefinition>>>,
    /// Default AWS region.
    pub default_region: String,
    /// Default AWS account ID.
    pub default_account_id: String,
    /// Internal event bus for cross-service fan-out (SNS→SQS, etc.).
    pub event_bus: EventBus,
    /// Total number of AWS API requests handled since startup.
    pub request_count: Arc<AtomicU64>,
    /// Server startup time.
    pub start_time: std::time::Instant,
}

impl AppState {
    pub fn new(default_region: String, default_account_id: String) -> Self {
        Self {
            services: Arc::new(HashMap::new()),
            routes: Arc::new(HashMap::new()),
            default_region,
            default_account_id,
            event_bus: EventBus::new(),
            request_count: Arc::new(AtomicU64::new(0)),
            start_time: std::time::Instant::now(),
        }
    }

    /// Register a service handler.
    pub fn register(&mut self, handler: Arc<dyn ServiceHandler>, routes: Vec<RouteDefinition>) {
        let signing_name = handler.signing_name().to_string();
        let service_name = handler.service_name().to_string();

        info!(
            service = %service_name,
            signing_name = %signing_name,
            protocol = ?handler.protocol(),
            routes = routes.len(),
            "Registered service"
        );

        Arc::get_mut(&mut self.services)
            .unwrap()
            .insert(signing_name.clone(), handler);

        if !routes.is_empty() {
            Arc::get_mut(&mut self.routes)
                .unwrap()
                .insert(signing_name, routes);
        }
    }
}

/// Main request handler — all AWS API requests funnel through here.
pub async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    state.request_count.fetch_add(1, Ordering::Relaxed);

    let request_id = uuid::Uuid::new_v4().to_string();

    debug!(
        method = %method,
        uri = %uri,
        request_id = %request_id,
        "Incoming request"
    );

    match process_request(&state, &method, &uri, &headers, &body, &request_id).await {
        Ok((status, mut resp_headers, resp_body)) => {
            let mut builder = Response::builder().status(status);
            for (key, value) in resp_headers.drain() {
                if let Some(key) = key {
                    builder = builder.header(key, value);
                }
            }
            builder.body(Body::from(resp_body)).unwrap()
        }
        Err((protocol, error)) => {
            warn!(
                error_code = %error.code,
                error_message = %error.message,
                request_id = %request_id,
                "Request failed"
            );
            let (status, mut resp_headers, resp_body) =
                protocol::serialize_error(protocol, &error, &request_id);
            let mut builder = Response::builder().status(status);
            for (key, value) in resp_headers.drain() {
                if let Some(key) = key {
                    builder = builder.header(key, value);
                }
            }
            builder.body(Body::from(resp_body)).unwrap()
        }
    }
}

async fn process_request(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    request_id: &str,
) -> Result<(StatusCode, HeaderMap, Bytes), (Protocol, AwsError)> {
    // 1. Extract service identification from auth header
    let (service_name, region, account_id) = extract_service_info(state, headers, uri);

    // 2. Find the service handler
    let handler = state
        .services
        .get(&service_name)
        .ok_or_else(|| {
            let protocol = protocol::detect_protocol(headers, body).unwrap_or(Protocol::RestJson1);
            (
                protocol,
                AwsError::bad_request(
                    "UnknownService",
                    format!("Service '{service_name}' is not registered"),
                ),
            )
        })?;

    let protocol = handler.protocol();

    // 3. Determine effective protocol (use service's declared protocol if detection fails)
    let detected = protocol::detect_protocol(headers, body).unwrap_or(protocol);

    // 4. Get routes for REST protocols
    let empty_routes = Vec::new();
    let routes = state.routes.get(&service_name).unwrap_or(&empty_routes);

    // 5. Parse the request
    let parsed = protocol::parse_request(detected, method, uri, headers, body, routes)
        .map_err(|e| (detected, e))?;

    debug!(
        service = %service_name,
        operation = %parsed.operation,
        request_id = %request_id,
        "Dispatching operation"
    );

    // 6. Build request context
    let ctx = crate::router::RequestContext {
        account_id,
        region,
        service: service_name.clone(),
        access_key: None,
        request_id: request_id.to_string(),
        method: method.to_string(),
        uri: uri.to_string(),
        event_bus: Some(state.event_bus.clone()),
    };

    // 7. Dispatch to service handler
    let result = handler
        .handle(&parsed.operation, parsed.input, &ctx)
        .await
        .map_err(|e| (detected, e))?;

    // 8. Serialize response using the *detected* protocol so that the wire
    // format matches what the client expects.  A client that sends an
    // awsQuery (form-encoded) request expects an XML response, even if the
    // service declares AwsJson as its primary protocol.
    Ok(protocol::serialize_response(
        detected,
        &parsed.operation,
        &result,
        request_id,
    ))
}

/// Extract service name, region, and account ID from the request.
fn extract_service_info(
    state: &AppState,
    headers: &HeaderMap,
    _uri: &Uri,
) -> (String, String, String) {
    // Try Authorization header first
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(creds) = auth::parse_authorization(auth_header) {
            return (
                creds.service,
                creds.region,
                state.default_account_id.clone(),
            );
        }
    }

    // Try X-Amz-Target header (for awsJson services)
    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok()) {
        if let Some(service) = resolve_service_from_target(target) {
            return (
                service,
                state.default_region.clone(),
                state.default_account_id.clone(),
            );
        }
    }

    // Try Host header
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) {
        if let Some(service) = extract_service_from_host(host) {
            return (
                service,
                state.default_region.clone(),
                state.default_account_id.clone(),
            );
        }
    }

    // Fallback: log what we received so we can diagnose routing failures
    warn!(
        auth = ?headers.get("authorization").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        target = ?headers.get("x-amz-target").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        host = ?headers.get("host").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        "Could not determine service — falling back to 'unknown'"
    );
    (
        "unknown".to_string(),
        state.default_region.clone(),
        state.default_account_id.clone(),
    )
}

/// Map X-Amz-Target prefixes to service signing names.
fn resolve_service_from_target(target: &str) -> Option<String> {
    let prefix = target.split('.').next()?;
    let service = match prefix {
        // Core services
        p if p.starts_with("DynamoDB") => "dynamodb",
        p if p.starts_with("AmazonSQS") => "sqs",
        p if p.starts_with("AmazonSNS") => "sns",
        p if p.starts_with("TrentService") => "kms",
        p if p.starts_with("secretsmanager") => "secretsmanager",
        p if p.starts_with("AmazonSSM") => "ssm",
        p if p.starts_with("Logs") => "logs",
        p if p.starts_with("Kinesis") => "kinesis",
        p if p.starts_with("AWSStepFunctions") => "states",
        p if p.starts_with("AWSEvents") => "events",
        // Auth
        p if p.starts_with("AWSCognitoIdentityProviderService") => "cognito-idp",
        p if p.starts_with("AWSCognitoIdentityService") => "cognito-identity",
        // Containers
        p if p.starts_with("AmazonEC2ContainerServiceV2") => "ecs",
        p if p.starts_with("AmazonEC2ContainerRegistry") => "ecr",
        // Data/Analytics
        p if p.starts_with("AmazonAthena") => "athena",
        p if p.starts_with("AWSGlue") => "glue",
        // Security
        p if p.starts_with("CertificateManager") => "acm",
        p if p.starts_with("AWSWAF") => "wafv2",
        _ => return None,
    };
    Some(service.to_string())
}

/// Extract service name from Host header.
/// e.g., "s3.us-east-1.localhost" → "s3"
/// e.g., "sqs.us-east-1.amazonaws.com" → "sqs"
fn extract_service_from_host(host: &str) -> Option<String> {
    // Remove port
    let host = host.split(':').next().unwrap_or(host);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        let first = parts[0];
        // Skip if it looks like a bucket name (for S3 virtual-hosted style)
        if !first.contains('-') || ["s3", "sqs", "sns", "dynamodb", "lambda", "iam", "sts", "kms", "logs", "events", "states", "ssm", "secretsmanager", "execute-api", "cognito-idp", "cognito-identity"].contains(&first) {
            return Some(first.to_string());
        }
    }
    None
}
