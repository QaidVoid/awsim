use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use bytes::Bytes;
use tracing::{debug, info, warn};

use crate::ServiceHandler;
use crate::auth;
use crate::authz::AuthzEngine;
use crate::body_store::BodyStore;
use crate::error::AwsError;
use crate::events::EventBus;
use crate::protocol::{self, Protocol, RouteDefinition};
use crate::request_detail::{RequestDetail, RequestDetailStore, capture_body, capture_headers};
use crate::request_event::{RequestEvent, RequestEventBus};

#[derive(Clone)]
pub struct BodyStoreHandle {
    pub service_name: String,
    pub groups: Vec<String>,
    pub body_store: Arc<BodyStore>,
}

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
    /// IAM authorization engine — opt-in via AWSIM_IAM_ENFORCE=true.
    pub authz: Arc<AuthzEngine>,
    /// Per-service `BodyStore` handles, populated when persistence is enabled.
    pub body_stores: Arc<Vec<BodyStoreHandle>>,
    /// Persistence root directory, when persistence is enabled.
    pub data_dir: Option<Arc<std::path::PathBuf>>,
    /// Broadcast bus for per-request observability events (consumed by SSE).
    pub events: RequestEventBus,
    /// Ring buffer of recent per-request detail captures (headers + bodies).
    pub request_details: RequestDetailStore,
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
            authz: Arc::new(AuthzEngine::from_env()),
            body_stores: Arc::new(Vec::new()),
            data_dir: None,
            events: RequestEventBus::new(),
            request_details: RequestDetailStore::default(),
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

struct ProcessOk {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    operation: String,
}

struct ProcessMeta {
    service: String,
    region: String,
    account_id: String,
    access_key: Option<String>,
}

/// Main request handler — all AWS API requests funnel through here.
pub async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let (response, _id) = dispatch_request(&state, method, uri, headers, body).await;
    response
}

/// Same as `handle_request`, but takes the state by reference and returns
/// the generated request id alongside the response. Lets internal callers
/// (replay, etc.) drive the gateway pipeline without going through axum.
pub async fn dispatch_request(
    state: &AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (Response<Body>, String) {
    state.request_count.fetch_add(1, Ordering::Relaxed);

    let request_id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();
    let request_size = body.len() as u64;

    debug!(
        method = %method,
        uri = %uri,
        request_id = %request_id,
        "Incoming request"
    );

    let mut meta = ProcessMeta {
        service: String::new(),
        region: state.default_region.clone(),
        account_id: state.default_account_id.clone(),
        access_key: None,
    };

    let outcome = process_request(
        state,
        &method,
        &uri,
        &headers,
        &body,
        &request_id,
        &mut meta,
    )
    .await;

    let (status, resp_headers, resp_body, operation, error_code) = match outcome {
        Ok(ProcessOk {
            status,
            headers,
            body,
            operation,
        }) => (status, headers, body, Some(operation), None),
        Err((protocol, error)) => {
            warn!(
                error_code = %error.code,
                error_message = %error.message,
                request_id = %request_id,
                "Request failed"
            );
            let err_code = error.code.clone();
            let (status, resp_headers, resp_body) =
                protocol::serialize_error(protocol, &error, &request_id);
            (status, resp_headers, resp_body, None, Some(err_code))
        }
    };
    let status_code = status.as_u16();
    let response_size = resp_body.len() as u64;

    // Capture detail for the inspect drawer before the body is moved into
    // the response. Bodies are size-capped inside `capture_body`.
    let body_cap = state.request_details.body_cap();
    let detail = RequestDetail {
        id: request_id.clone(),
        method: method.to_string(),
        path: uri.path().to_string(),
        query: uri.query().map(|q| q.to_string()),
        status_code,
        request_headers: capture_headers(&headers),
        response_headers: capture_headers(&resp_headers),
        request_body: capture_body(&body, body_cap),
        response_body: capture_body(&resp_body, body_cap),
    };
    state.request_details.insert(detail);

    let mut builder = Response::builder().status(status);
    let mut resp_headers = resp_headers;
    for (key, value) in resp_headers.drain() {
        if let Some(key) = key {
            builder = builder.header(key, value);
        }
    }
    let response = builder.body(Body::from(resp_body)).unwrap();

    let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let principal_arn = meta
        .access_key
        .as_ref()
        .map(|ak| format!("arn:aws:iam::{}:access-key/{}", meta.account_id, ak));

    let event = RequestEvent {
        id: request_id.clone(),
        ts,
        method: method.to_string(),
        path: uri.path().to_string(),
        service: meta.service,
        operation,
        account_id: meta.account_id,
        region: meta.region,
        principal_arn,
        status_code,
        duration_ms,
        request_size,
        response_size,
        error_code,
    };
    state.events.publish(event);

    (response, request_id)
}

async fn process_request(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    request_id: &str,
    meta: &mut ProcessMeta,
) -> Result<ProcessOk, (Protocol, AwsError)> {
    // 1. Extract service identification from auth header
    let (service_name, region, account_id, access_key) = extract_service_info(state, headers, uri);
    meta.service = service_name.clone();
    meta.region = region.clone();
    meta.account_id = account_id.clone();
    meta.access_key = access_key.clone();

    // 2. Find the service handler
    let handler = state.services.get(&service_name).ok_or_else(|| {
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
        access_key,
        request_id: request_id.to_string(),
        method: method.to_string(),
        uri: uri.to_string(),
        event_bus: Some(state.event_bus.clone()),
    };

    // 6b. IAM authorization (opt-in via AWSIM_IAM_ENFORCE)
    if let (Some(action), Some(resource)) = (
        handler.iam_action(&parsed.operation),
        handler.iam_resource(&parsed.operation, &parsed.input, &ctx),
    ) {
        state
            .authz
            .check(&ctx, &action, &resource)
            .map_err(|e| (detected, e))?;
    } else {
        debug!(
            service = %service_name,
            operation = %parsed.operation,
            "Skipping IAM check — handler does not declare action/resource"
        );
    }

    let operation = parsed.operation.clone();

    // 7. Dispatch to service handler
    let result = handler
        .handle(&parsed.operation, parsed.input, &ctx)
        .await
        .map_err(|e| (detected, e))?;

    // 8. Serialize response using the *detected* protocol so that the wire
    // format matches what the client expects.  A client that sends an
    // awsQuery (form-encoded) request expects an XML response, even if the
    // service declares AwsJson as its primary protocol.
    let (status, headers, body) =
        protocol::serialize_response(detected, &parsed.operation, &result, request_id);
    Ok(ProcessOk {
        status,
        headers,
        body,
        operation,
    })
}

/// Extract service name, region, account ID, and access key from the request.
fn extract_service_info(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> (String, String, String, Option<String>) {
    // Try Authorization header first
    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(creds) = auth::parse_authorization(auth_header)
    {
        return (
            creds.service,
            creds.region,
            state.default_account_id.clone(),
            Some(creds.access_key),
        );
    }

    // Try X-Amz-Target header (for awsJson services)
    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok())
        && let Some(service) = resolve_service_from_target(target)
    {
        return (
            service,
            state.default_region.clone(),
            state.default_account_id.clone(),
            None,
        );
    }

    // Try Host header
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok())
        && let Some(service) = extract_service_from_host(host)
    {
        return (
            service,
            state.default_region.clone(),
            state.default_account_id.clone(),
            None,
        );
    }

    // Check for pre-signed URL query parameters (SigV4 in query string)
    if let Some(query) = uri.query()
        && query.contains("X-Amz-Credential")
        && let Some(cred_start) = query.find("X-Amz-Credential=")
    {
        let cred_val = &query[cred_start + 17..];
        let cred_end = cred_val.find('&').unwrap_or(cred_val.len());
        let cred = &cred_val[..cred_end];
        let cred_decoded = cred.replace("%2F", "/");
        let parts: Vec<&str> = cred_decoded.split('/').collect();
        if parts.len() >= 4 {
            return (
                parts[3].to_string(),
                parts[2].to_string(),
                state.default_account_id.clone(),
                Some(parts[0].to_string()),
            );
        }
    }

    // Try path-based detection as last resort (for REST services called without auth)
    let path = uri.path();
    if let Some(service) = resolve_service_from_path(path) {
        return (
            service,
            state.default_region.clone(),
            state.default_account_id.clone(),
            None,
        );
    }

    // Fallback: log what we received so we can diagnose routing failures
    warn!(
        auth = ?headers.get("authorization").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        target = ?headers.get("x-amz-target").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        host = ?headers.get("host").map(|v| v.to_str().unwrap_or("<non-utf8>")),
        path = %path,
        "Could not determine service — falling back to 'unknown'"
    );
    (
        "unknown".to_string(),
        state.default_region.clone(),
        state.default_account_id.clone(),
        None,
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
        p if p.starts_with("Comprehend") => "comprehend",
        p if p.starts_with("kendra") => "kendra",
        // Management & audit
        p if p.starts_with("AWSOrganizationsV") => "organizations",
        p if p.starts_with("CloudTrail_") => "cloudtrail",
        // Streaming
        p if p.starts_with("Firehose_") => "firehose",
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
        if !first.contains('-')
            || [
                "s3",
                "sqs",
                "sns",
                "dynamodb",
                "lambda",
                "iam",
                "sts",
                "kms",
                "logs",
                "events",
                "states",
                "ssm",
                "secretsmanager",
                "execute-api",
                "cognito-idp",
                "cognito-identity",
            ]
            .contains(&first)
        {
            return Some(first.to_string());
        }
    }
    None
}

/// Last-resort: guess the service from the URI path pattern.
/// This handles REST-protocol services when no auth header is present
/// (e.g., requests from the admin console).
fn resolve_service_from_path(path: &str) -> Option<String> {
    let service = match path {
        // Lambda
        p if p.starts_with("/2015-03-31/functions") || p.starts_with("/2018-10-31/layers") => {
            "lambda"
        }
        // API Gateway v2
        p if p.starts_with("/v2/apis") => "execute-api",
        // SES v2
        p if p.starts_with("/v2/email") => "ses",
        // Route53
        p if p.starts_with("/2013-04-01/hostedzone")
            || p.starts_with("/2013-04-01/healthcheck")
            || p.starts_with("/2013-04-01/tags") =>
        {
            "route53"
        }
        // CloudFront
        p if p.starts_with("/2020-05-31/distribution")
            || p.starts_with("/2020-05-31/origin-access-control")
            || p.starts_with("/2020-05-31/cache-policy")
            || p.starts_with("/2020-05-31/tagging") =>
        {
            "cloudfront"
        }
        // AppSync
        p if p.starts_with("/v1/apis") => "appsync",
        // Bedrock
        p if p.starts_with("/foundation-models")
            || p.starts_with("/guardrails")
            || p.starts_with("/model-customization") =>
        {
            "bedrock"
        }
        // Bedrock Runtime
        p if p.starts_with("/model/") => "bedrock-runtime",
        // EventBridge Scheduler
        p if p.starts_with("/schedules") || p.starts_with("/schedule-groups") => "scheduler",
        // EKS
        p if p.starts_with("/clusters") || p == "/tags" || p.starts_with("/tags/") => "eks",
        // S3 (catch-all — any path starting with / that doesn't match above could be S3)
        // Don't add S3 here as it would catch everything
        _ => return None,
    };
    Some(service.to_string())
}
