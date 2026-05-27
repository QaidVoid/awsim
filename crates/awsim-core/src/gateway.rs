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
use crate::request_detail::{
    CapturedBody, RequestDetail, RequestDetailStore, capture_body, capture_headers,
};
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
    /// Default AWS partition (`aws`, `aws-cn`, `aws-us-gov`, ...).
    pub default_partition: String,
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
    /// Chaos engine - empty by default, populated by admin endpoints.
    /// Evaluated before dispatch to inject synthetic errors / latency.
    pub chaos: Arc<awsim_chaos::ChaosEngine>,
    /// Worker pool for slow background tasks (rotation invocation,
    /// scheduler dispatch, ESM polling, etc.). Services enqueue
    /// futures here instead of spawning ad-hoc tokio tasks so we get
    /// one bounded place to drain on shutdown.
    pub workers: crate::tick::WorkerPool,
}

impl AppState {
    pub fn new(default_region: String, default_account_id: String) -> Self {
        Self::with_partition(
            default_region,
            default_account_id,
            crate::router::DEFAULT_PARTITION.to_string(),
        )
    }

    /// Construct an `AppState` with a non-default AWS partition.
    /// Use this for `aws-cn`, `aws-us-gov`, or `aws-iso(-b)` deployments
    /// so emitted ARNs match the configured partition.
    pub fn with_partition(
        default_region: String,
        default_account_id: String,
        default_partition: String,
    ) -> Self {
        Self {
            services: Arc::new(HashMap::new()),
            routes: Arc::new(HashMap::new()),
            default_region,
            default_account_id,
            default_partition,
            event_bus: EventBus::new(),
            request_count: Arc::new(AtomicU64::new(0)),
            start_time: std::time::Instant::now(),
            authz: Arc::new(AuthzEngine::from_env()),
            body_stores: Arc::new(Vec::new()),
            data_dir: None,
            events: RequestEventBus::new(),
            request_details: RequestDetailStore::default(),
            chaos: Arc::new(awsim_chaos::ChaosEngine::new()),
            workers: crate::tick::WorkerPool::new(),
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
    body: ProcessBody,
    operation: String,
}

/// Response payload returned from request processing. Most operations
/// produce a single buffered `Bytes` payload; streaming endpoints
/// (Bedrock event-stream APIs) return an open stream of chunks the
/// gateway forwards via chunked HTTP transfer.
enum ProcessBody {
    Bytes(Bytes),
    Stream(crate::HandlerByteStream),
}

impl ProcessBody {
    fn buffered_len(&self) -> Option<usize> {
        match self {
            ProcessBody::Bytes(b) => Some(b.len()),
            ProcessBody::Stream(_) => None,
        }
    }
}

struct ProcessMeta {
    service: String,
    region: String,
    account_id: String,
    access_key: Option<String>,
}

/// Main request handler — all AWS API requests funnel through here.
/// Spawn the periodic tick loop. Runs in the background until the
/// process exits, calling `tick` on every registered service every
/// `interval`. Per the [`ServiceHandler::tick`] contract, individual
/// services must keep each tick under ~10 ms — slow work is enqueued
/// elsewhere so this loop stays responsive.
///
/// Returns the [`tokio::task::JoinHandle`] so callers can cancel the
/// loop on shutdown if they want to.
/// Cached once-per-process read of `AWSIM_REQUIRE_SIGNED_REQUESTS`.
fn require_signed_requests_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("AWSIM_REQUIRE_SIGNED_REQUESTS")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
    })
}

pub fn spawn_tick_loop(
    state: AppState,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;

    /// Upper bound on how long a single service's `tick` is allowed
    /// to take before the loop moves on. Tick is documented as a
    /// fast-path (sub-10ms typical); anything longer indicates the
    /// service is doing work that belongs in the worker pool.
    const PER_SERVICE_TICK_DEADLINE: std::time::Duration = std::time::Duration::from_millis(50);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick so a slow startup doesn't ripple
        // into a tick storm if the loop falls behind.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // consume the immediate tick
        loop {
            ticker.tick().await;
            // Snapshot the service list so a service registering / removing
            // mid-loop doesn't disturb iteration.
            let services: Vec<(String, Arc<dyn ServiceHandler>)> = state
                .services
                .iter()
                .map(|(name, svc)| (name.clone(), svc.clone()))
                .collect();
            for (name, svc) in services {
                let tick_fut = AssertUnwindSafe(svc.tick()).catch_unwind();
                match tokio::time::timeout(PER_SERVICE_TICK_DEADLINE, tick_fut).await {
                    Ok(Ok(())) => {}
                    Ok(Err(panic)) => {
                        let msg = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&'static str>().map(|s| s.to_string()))
                            .unwrap_or_else(|| "<non-string panic payload>".to_string());
                        warn!(service = %name, panic = %msg, "service tick panicked");
                    }
                    Err(_) => {
                        warn!(
                            service = %name,
                            budget_ms = PER_SERVICE_TICK_DEADLINE.as_millis() as u64,
                            "service tick exceeded budget; consider moving slow work to AppState::workers"
                        );
                    }
                }
            }
        }
    })
}

pub async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    // Short-circuit common browser probes (favicon, devtools well-known
    // path) before the AWS dispatch pipeline runs. They are not API
    // calls — silently 204'ing keeps them out of the request log and
    // out of the inspect drawer.
    if is_browser_probe(&method, uri.path()) {
        return Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap();
    }
    let (response, _id) = dispatch_request(&state, method, uri, headers, body).await;
    response
}

/// Recognise the handful of unsolicited paths browsers hit when you point
/// them at AWSim's port — they're not AWS requests and shouldn't appear
/// in logs or stats. Conservative on purpose: only paths we've actually
/// seen in real traces are listed.
fn is_browser_probe(method: &Method, path: &str) -> bool {
    if method != Method::GET {
        return false;
    }
    matches!(
        path,
        "/favicon.ico"
            | "/apple-touch-icon.png"
            | "/apple-touch-icon-precomposed.png"
            | "/robots.txt"
            | "/.well-known/appspecific/com.chrome.devtools.json"
    )
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

    let (status, mut resp_headers, resp_body, operation, error_code) = match outcome {
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
            (
                status,
                resp_headers,
                ProcessBody::Bytes(resp_body),
                None,
                Some(err_code),
            )
        }
    };
    let status_code = status.as_u16();
    // Streaming responses don't have a known length up front and we
    // never buffer them — report 0 for now (they show up in the log
    // with a fixed marker).
    let response_size = resp_body.buffered_len().unwrap_or(0) as u64;

    // Capture detail for the inspect drawer. Streaming bodies bypass
    // capture (no full payload to store); buffered bodies go through
    // the existing size-capped path.
    let body_cap = state.request_details.body_cap();
    let captured_response = match &resp_body {
        ProcessBody::Bytes(b) => capture_body(b, body_cap),
        ProcessBody::Stream(_) => CapturedBody::placeholder("<streaming response>"),
    };
    let detail = RequestDetail {
        id: request_id.clone(),
        method: method.to_string(),
        path: uri.path().to_string(),
        query: uri.query().map(|q| q.to_string()),
        status_code,
        request_headers: capture_headers(&headers),
        response_headers: capture_headers(&resp_headers),
        request_body: capture_body(&body, body_cap),
        response_body: captured_response,
    };
    state.request_details.insert(detail);

    // Extract any X-Awsim-* metadata headers the responding service
    // attached for the billing meter (e.g. Lambda's per-invocation
    // memory size, Step Functions' transition count). Pull them off
    // before draining into the actual HTTP response so they don't
    // leak to the wire.
    let memory_mb = resp_headers
        .remove("x-awsim-memory-mb")
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<u32>().ok()));
    let state_transitions = resp_headers
        .remove("x-awsim-state-transitions")
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<u32>().ok()));
    let character_count = resp_headers
        .remove("x-awsim-char-count")
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<u64>().ok()));

    let mut builder = Response::builder().status(status);
    for (key, value) in resp_headers.drain() {
        if let Some(key) = key {
            builder = builder.header(key, value);
        }
    }
    let body_for_response = match resp_body {
        ProcessBody::Bytes(b) => Body::from(b),
        ProcessBody::Stream(s) => {
            // Wrap each chunk as a Frame so axum can stream it via
            // chunked transfer. The error case yields the AWS error
            // message inline so a downstream parse can surface it
            // (the connection has already been opened with HTTP 200,
            // so we can't switch to a 5xx mid-stream).
            use futures::StreamExt;
            let mapped = s.map(|res| match res {
                Ok(b) => Ok::<_, std::io::Error>(b),
                Err(e) => {
                    let payload = format!("{{\"error\":\"{}\"}}", e.message);
                    Ok(Bytes::from(payload))
                }
            });
            Body::from_stream(mapped)
        }
    };
    let response = builder.body(body_for_response).unwrap();

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
        service: meta.service.clone(),
        operation: operation.clone(),
        account_id: meta.account_id.clone(),
        region: meta.region.clone(),
        principal_arn: principal_arn.clone(),
        status_code,
        duration_ms,
        request_size,
        response_size,
        error_code: error_code.clone(),
        memory_mb,
        state_transitions,
        character_count,
    };
    state.events.publish(event);

    // Per-API-call event on the cross-service bus, in the canonical
    // shape CloudTrail / EventBridge / AWS Config consumers expect.
    // Empty service name happens for malformed requests that never
    // resolved a handler; skip those - they're not API calls.
    if !meta.service.is_empty() {
        let user_agent = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let api_event = crate::events::ApiCallDetail {
            event_id: request_id.clone(),
            event_source: format!("{}.amazonaws.com", meta.service),
            event_name: operation.clone().unwrap_or_default(),
            event_time_epoch: ts,
            source_ip: state.request_details.get(&request_id).and_then(|d| {
                d.request_headers.iter().find_map(|h| {
                    if h.name.eq_ignore_ascii_case("x-forwarded-for") {
                        Some(h.value.clone())
                    } else {
                        None
                    }
                })
            }),
            user_agent,
            user_identity_arn: principal_arn,
            user_identity_account: Some(meta.account_id.clone()),
            request_parameters: None,
            response_elements: None,
            error_code,
            error_message: None,
            http_status: status_code,
        };
        state
            .event_bus
            .publish_api_call(meta.region, meta.account_id, api_event);
    }

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
    let (mut service_name, region, account_id, access_key) =
        extract_service_info(state, headers, uri);
    meta.region = region.clone();
    meta.account_id = account_id.clone();
    meta.access_key = access_key.clone();

    // 1b. Enforce signed-request mode when configured. The default
    // stays permissive so existing tests / dev workflows aren't
    // broken; flipping AWSIM_REQUIRE_SIGNED_REQUESTS=true makes the
    // gateway require every call to carry a SigV4 signature whose
    // access key resolves to a known IAM principal. Otherwise the
    // call is rejected with the same InvalidClientTokenId AWS
    // returns for stale or unknown keys.
    if require_signed_requests_enabled() {
        let protocol = protocol::detect_protocol(headers, body).unwrap_or(Protocol::RestJson1);
        match access_key.as_deref() {
            None => {
                return Err((
                    protocol,
                    AwsError::bad_request(
                        "MissingAuthenticationTokenException",
                        "Request must be signed; no Authorization header found.",
                    ),
                ));
            }
            Some(key)
                if !state.authz.is_admin_access_key(key)
                    && state
                        .authz
                        .principal_lookup
                        .resolve_access_key(key)
                        .is_none() =>
            {
                return Err((
                    protocol,
                    AwsError::bad_request(
                        "InvalidClientTokenId",
                        "The security token included in the request is invalid.",
                    ),
                ));
            }
            _ => {}
        }
    }

    // 1c. Cryptographic SigV4 verification when AWSIM_VERIFY_SIGV4
    // is on. The presence-check above only confirmed the access key
    // exists; this step recomputes the signature with the matching
    // secret and rejects mismatches. Off by default so legacy
    // clients sending "Signature=fakesignature" keep working.
    if crate::sigv4_verify::verify_enabled()
        && let Some(key) = access_key.as_deref()
        && !state.authz.is_admin_access_key(key)
    {
        let protocol = protocol::detect_protocol(headers, body).unwrap_or(Protocol::RestJson1);
        if let Err(err) = verify_signature_for_request(state, headers, method, uri, body, key) {
            return Err((protocol, err));
        }
    }

    // 1d. Auth hook: record the access key's usage for
    // `GetAccessKeyLastUsed`. AWS slides the timestamp on every
    // successful authenticated call; the principal-lookup default impl
    // is a no-op so non-IAM lookups (tests, Cognito identity) skip the
    // bookkeeping silently.
    if let Some(key) = access_key.as_deref()
        && !key.is_empty()
        && !state.authz.is_admin_access_key(key)
    {
        state
            .authz
            .principal_lookup
            .record_access_key_used(key, &service_name, &region);
    }

    // 2. Find the service handler
    let mut handler = state.services.get(&service_name).ok_or_else(|| {
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
    let mut detected = protocol::detect_protocol(headers, body).unwrap_or(protocol);

    // 4. Get routes for REST protocols
    let empty_routes = Vec::new();
    let routes = state.routes.get(&service_name).unwrap_or(&empty_routes);

    // 5. Parse the request. If the auth-derived service has no
    // matching route, fall back to path-based service detection
    // before erroring. This is the magic that makes tools like
    // Vercel's `@ai-sdk/amazon-bedrock` work — when pointed at a
    // localhost endpoint the underlying signer can't infer the
    // right service from the hostname (no `bedrock-runtime.`
    // subdomain), so it signs with `bedrock` (control plane) even
    // though the path is a `bedrock-runtime` data-plane operation.
    // Routing strictly by the auth-signed service would surface as
    // `UnknownOperationException` here. Instead we look at the
    // path: if it uniquely identifies a different registered
    // service AND that service knows how to handle this route, we
    // switch over and dispatch there.
    let parsed = match protocol::parse_request(detected, method, uri, headers, body, routes) {
        Ok(p) => p,
        Err(e) if e.code == "UnknownOperationException" => {
            if let Some(path_service) = resolve_service_from_path(uri.path())
                && path_service != service_name
                && let Some(fallback_handler) = state.services.get(&path_service)
            {
                let fallback_routes = state.routes.get(&path_service).unwrap_or(&empty_routes);
                let fallback_protocol = fallback_handler.protocol();
                let fallback_detected =
                    protocol::detect_protocol(headers, body).unwrap_or(fallback_protocol);
                match protocol::parse_request(
                    fallback_detected,
                    method,
                    uri,
                    headers,
                    body,
                    fallback_routes,
                ) {
                    Ok(p) => {
                        debug!(
                            auth_service = %service_name,
                            path_service = %path_service,
                            "Auth-derived service had no matching route; falling back to path-derived service"
                        );
                        service_name = path_service;
                        handler = fallback_handler;
                        detected = fallback_detected;
                        // `protocol` and `routes` are unused after
                        // parse completes; no need to update them.
                        let _ = fallback_protocol;
                        let _ = fallback_routes;
                        p
                    }
                    Err(_) => return Err((detected, e)),
                }
            } else {
                return Err((detected, e));
            }
        }
        Err(e) => return Err((detected, e)),
    };
    meta.service = service_name.clone();

    debug!(
        service = %service_name,
        operation = %parsed.operation,
        request_id = %request_id,
        "Dispatching operation"
    );

    // 6. Build request context. Source IP and TLS marker come from the
    // `X-Forwarded-For` / `X-Forwarded-Proto` headers a fronting proxy
    // (or our own dev shim) sets; without them we leave `source_ip = None`
    // so policies that gate on `aws:SourceIp` simply can't match rather
    // than matching a fake `0.0.0.0`.
    let source_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let is_secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    let ctx = crate::router::RequestContext {
        account_id,
        region,
        partition: state.default_partition.clone(),
        service: service_name.clone(),
        access_key,
        request_id: request_id.to_string(),
        method: method.to_string(),
        uri: uri.to_string(),
        event_bus: Some(state.event_bus.clone()),
        source_ip,
        is_secure,
        // External request from the gateway: bypass is never set
        // here. Only server-internal flows like bootstrap setup may
        // construct a context with `internal_bypass = true`.
        internal_bypass: false,
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

    // 6c. Chaos injection — sleep + optionally short-circuit with a
    // synthetic AWS error before the handler runs. Empty engine is a
    // no-op fast path so the cost is negligible when chaos is off.
    if let Some(outcome) = state.chaos.evaluate(&service_name, Some(&operation)) {
        if let Some(delay) = outcome.latency {
            tokio::time::sleep(delay).await;
        }
        state
            .chaos
            .record_injection(&outcome.rule_id, &service_name, Some(&operation));
        if let Some(err) = outcome.error {
            let status = axum::http::StatusCode::from_u16(err.status)
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            let error_type = if status.is_server_error() {
                crate::error::ErrorType::Receiver
            } else {
                crate::error::ErrorType::Sender
            };
            let aws_err = AwsError {
                status,
                code: err.code,
                message: err.message,
                error_type,
                extras: None,
            };
            return Err((detected, aws_err));
        }
    }

    // 7. Dispatch to service handler. Use `handle_streaming` so the
    // handler can opt into chunked transfer when it has a real
    // streaming source (e.g. Bedrock proxying Ollama's SSE).
    let handler_result = handler
        .handle_streaming(&parsed.operation, parsed.input, &ctx)
        .await
        .map_err(|e| (detected, e))?;

    // 8. Build the response. Streaming results bypass the per-protocol
    // serializer — the handler has already encoded the bytes (e.g.
    // AWS event-stream binary frames) and supplied the wire-level
    // content-type. For everything else, serialize the JSON
    // response using the detected protocol so the wire format
    // matches what the client expects (an awsQuery client gets XML
    // back even if the service is awsJson-native, etc.).
    match handler_result {
        crate::HandlerResult::Streaming { body, content_type } => {
            let mut headers = HeaderMap::new();
            if let Ok(v) = content_type.parse() {
                headers.insert(axum::http::header::CONTENT_TYPE, v);
            }
            if let Ok(v) = request_id.parse() {
                headers.insert("x-amzn-requestid", v);
            }
            Ok(ProcessOk {
                status: StatusCode::OK,
                headers,
                body: ProcessBody::Stream(body),
                operation,
            })
        }
        crate::HandlerResult::Json(value) => {
            let (status, headers, body) =
                protocol::serialize_response(detected, &parsed.operation, &value, request_id);
            Ok(ProcessOk {
                status,
                headers,
                body: ProcessBody::Bytes(body),
                operation,
            })
        }
    }
}

/// Extract service name, region, account ID, and access key from the request.
/// Reconstruct the canonical request from the inbound axum pieces and
/// hand it to `sigv4_verify::verify` along with the secret bound to
/// the caller's access key. Returns a translated AWS-style error on
/// rejection so the gateway can wrap it with the request's protocol.
fn verify_signature_for_request(
    state: &AppState,
    headers: &HeaderMap,
    method: &Method,
    uri: &Uri,
    body: &Bytes,
    access_key: &str,
) -> Result<(), AwsError> {
    // Presigned URLs carry the signature in the query string, not the
    // Authorization header. Detect the presence of X-Amz-Signature and
    // route those through the presigned verifier before falling back
    // to the header-based path.
    if let Some(query) = uri.query()
        && query.contains("X-Amz-Signature=")
    {
        return verify_presigned_for_request(state, headers, method, uri, access_key);
    }

    let auth_value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AwsError::bad_request(
                "MissingAuthenticationTokenException",
                "Request must be signed with SigV4 when AWSIM_VERIFY_SIGV4 is on.",
            )
        })?;
    let auth = crate::sigv4_verify::parse_authorization_header(auth_value).ok_or_else(|| {
        AwsError::bad_request(
            "IncompleteSignatureException",
            "Authorization header is not in the expected AWS4-HMAC-SHA256 shape.",
        )
    })?;
    let amz_date = headers
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AwsError::bad_request(
                "IncompleteSignatureException",
                "x-amz-date header is required for SigV4-signed requests.",
            )
        })?;
    let payload_hash_header = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok());
    let secret = state
        .authz
        .principal_lookup
        .resolve_secret(access_key)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidClientTokenId",
                "The security token included in the request is invalid.",
            )
        })?;

    // Pull the headers listed in SignedHeaders out of the inbound
    // request in the same order. Missing entries make the signature
    // unrecoverable, so reject early.
    let mut headers_for_canonical: Vec<(String, String)> =
        Vec::with_capacity(auth.signed_headers.len());
    for name in &auth.signed_headers {
        let lower = name.to_ascii_lowercase();
        let value = if lower == "host" {
            headers
                .get("host")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .or_else(|| uri.host().map(|s| s.to_string()))
                .unwrap_or_default()
        } else {
            headers
                .get(lower.as_str())
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };
        headers_for_canonical.push((lower, value));
    }

    let path = uri.path();
    let query = uri.query().unwrap_or("");
    // The query string already canonicalises easily because it comes
    // off the wire in encoded form; for SigV4 we still must sort the
    // key/value pairs.
    let canonical_query = canonicalize_query(query);

    let outcome = crate::sigv4_verify::verify(
        &auth,
        &secret,
        method.as_str(),
        path,
        &canonical_query,
        &headers_for_canonical,
        amz_date,
        body,
        payload_hash_header,
        std::time::SystemTime::now(),
        std::time::Duration::from_secs(300),
    );
    match outcome {
        crate::sigv4_verify::VerifyOutcome::Ok => Ok(()),
        crate::sigv4_verify::VerifyOutcome::IncompleteSignature => Err(AwsError::bad_request(
            "IncompleteSignatureException",
            "SigV4 verification failed: required header missing.",
        )),
        crate::sigv4_verify::VerifyOutcome::SignatureMismatch => Err(AwsError::forbidden(
            "SignatureDoesNotMatch",
            "The request signature we calculated does not match the signature you provided.",
        )),
    }
}

/// Verify a presigned URL's SigV4 signature.
///
/// The caller has already detected that the URL carries an
/// `X-Amz-Signature` query parameter. We pull the access key from the
/// `X-Amz-Credential` parameter, look up its secret, then hand the raw
/// query string + path + host header to
/// [`sigv4_verify::verify_presigned`]. Rejects with the AWS-standard
/// `SignatureDoesNotMatch` / `IncompleteSignatureException` on
/// mismatch or missing pieces.
fn verify_presigned_for_request(
    state: &AppState,
    headers: &HeaderMap,
    method: &Method,
    uri: &Uri,
    access_key: &str,
) -> Result<(), AwsError> {
    let raw_query = uri.query().unwrap_or("");
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| uri.host().map(|s| s.to_string()))
        .unwrap_or_default();
    let secret = state
        .authz
        .principal_lookup
        .resolve_secret(access_key)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidClientTokenId",
                "The security token included in the request is invalid.",
            )
        })?;
    let outcome = crate::sigv4_verify::verify_presigned(
        method.as_str(),
        uri.path(),
        raw_query,
        &host,
        &secret,
        std::time::SystemTime::now(),
        std::time::Duration::from_secs(300),
    );
    match outcome {
        crate::sigv4_verify::VerifyOutcome::Ok => Ok(()),
        crate::sigv4_verify::VerifyOutcome::IncompleteSignature => Err(AwsError::bad_request(
            "IncompleteSignatureException",
            "Presigned URL is missing one of X-Amz-Algorithm / X-Amz-Credential / X-Amz-Date / X-Amz-SignedHeaders / X-Amz-Signature.",
        )),
        crate::sigv4_verify::VerifyOutcome::SignatureMismatch => Err(AwsError::forbidden(
            "SignatureDoesNotMatch",
            "The request signature we calculated does not match the signature you provided.",
        )),
    }
}

fn canonicalize_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut parts: Vec<(String, String)> = query
        .split('&')
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (kv.to_string(), String::new()),
        })
        .collect();
    parts.sort();
    parts
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn extract_service_info(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> (String, String, String, Option<String>) {
    // 1. Authorization header — SigV4-signed direct calls.
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

    // 2. X-Amz-Credential query string — presigned URLs (S3 GetObject /
    //    PutObject, CloudFront, ...). Must beat Host detection because
    //    the URL host is whatever bucket / CDN front-end the SDK
    //    chose, which can be misleading: a presigned PUT against a
    //    custom endpoint host like `aws.qaidvoid.dev` would otherwise
    //    have its host-derived service ("aws") win over the real
    //    `s3` baked into the credential scope, and S3's request
    //    handler would never see the upload.
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

    // 3. X-Amz-Target header — awsJson services (DynamoDB, Cognito, ...).
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

    // 4. Host header — convention-based, only trusted when the host's
    //    leftmost segment matches a registered service signing name.
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok())
        && let Some(service) = extract_service_from_host(host, state)
    {
        return (
            service,
            state.default_region.clone(),
            state.default_account_id.clone(),
            None,
        );
    }

    // 5. Path-based detection — last resort for unsigned REST calls
    //    (admin console, health probes, ...).
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
        // Cross-service tagging
        p if p.starts_with("ResourceGroupsTaggingAPI") => "tagging",
        // Auto scaling
        p if p.starts_with("AnyScaleFrontendService") => "application-autoscaling",
        // Cloud Map (Service Discovery)
        p if p.starts_with("Route53AutoNaming_v") => "servicediscovery",
        // MemoryDB
        p if p.starts_with("AmazonMemoryDB") => "memorydb",
        _ => return None,
    };
    Some(service.to_string())
}

/// Extract service name from Host header by walking dot-segments
/// left-to-right and returning the first one that matches a
/// registered service's signing name.
///
/// Examples:
///   `s3.us-east-1.amazonaws.com`     → `s3`
///   `sqs.us-east-1.localhost`        → `sqs`
///   `sqs.us-east-1.aws.qaidvoid.dev` → `sqs`
///   `aws.qaidvoid.dev`               → `None`  (no service segment)
///   `localhost:4566`                 → `None`  (no service segment)
///
/// The earlier hard-coded allowlist + "skip if first contains a
/// dash" heuristic falsely returned `aws` for the bundled
/// `aws.qaidvoid.dev` cert host - which then beat
/// `X-Amz-Credential` parsing and broke S3 presigned uploads.
/// Anchoring on `state.services` (whose keys are the signing names
/// every registered handler advertises) keeps this check honest
/// without a parallel allowlist to maintain.
fn extract_service_from_host(host: &str, state: &AppState) -> Option<String> {
    let host = host.split(':').next().unwrap_or(host);
    for part in host.split('.') {
        if state.services.contains_key(part) {
            return Some(part.to_string());
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

#[cfg(test)]
mod browser_probe_tests {
    use super::*;

    #[test]
    fn matches_known_probes() {
        assert!(is_browser_probe(&Method::GET, "/favicon.ico"));
        assert!(is_browser_probe(
            &Method::GET,
            "/.well-known/appspecific/com.chrome.devtools.json"
        ));
        assert!(is_browser_probe(&Method::GET, "/robots.txt"));
        assert!(is_browser_probe(&Method::GET, "/apple-touch-icon.png"));
    }

    #[test]
    fn ignores_non_get_methods() {
        // S3 PutObject to /favicon.ico would be a real (if weird) call.
        assert!(!is_browser_probe(&Method::PUT, "/favicon.ico"));
        assert!(!is_browser_probe(&Method::POST, "/favicon.ico"));
    }

    #[test]
    fn ignores_unknown_paths() {
        assert!(!is_browser_probe(&Method::GET, "/"));
        assert!(!is_browser_probe(&Method::GET, "/some-bucket/key"));
        assert!(!is_browser_probe(&Method::GET, "/_awsim/stats"));
    }
}

#[cfg(test)]
mod tick_tests {
    use super::*;
    use crate::RequestContext;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// Test handler that counts tick invocations.
    struct CountingService {
        ticks: Arc<AtomicU64>,
    }

    #[async_trait::async_trait]
    impl ServiceHandler for CountingService {
        fn service_name(&self) -> &str {
            "test"
        }
        fn protocol(&self) -> Protocol {
            Protocol::AwsJson1_1
        }
        async fn handle(
            &self,
            _: &str,
            _: serde_json::Value,
            _: &RequestContext,
        ) -> Result<serde_json::Value, AwsError> {
            Ok(serde_json::Value::Null)
        }
        async fn tick(&self) {
            self.ticks.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn tick_loop_invokes_each_registered_service() {
        let counter = Arc::new(AtomicU64::new(0));
        let svc = Arc::new(CountingService {
            ticks: counter.clone(),
        }) as Arc<dyn ServiceHandler>;

        let mut services: HashMap<String, Arc<dyn ServiceHandler>> = HashMap::new();
        services.insert("test".to_string(), svc);

        let mut state = AppState::new("us-east-1".to_string(), "000000000000".to_string());
        state.services = Arc::new(services);

        let handle = spawn_tick_loop(state, Duration::from_millis(50));
        // Wait long enough for ~3 ticks.
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();

        let count = counter.load(Ordering::SeqCst);
        assert!(count >= 2, "expected at least 2 ticks, got {count}");
    }

    /// Service whose `tick` panics every time. Used to verify the
    /// tick loop catches the panic and keeps invoking other
    /// services.
    struct PanickingService;

    #[async_trait::async_trait]
    impl ServiceHandler for PanickingService {
        fn service_name(&self) -> &str {
            "panicky"
        }
        fn protocol(&self) -> Protocol {
            Protocol::AwsJson1_1
        }
        async fn handle(
            &self,
            _: &str,
            _: serde_json::Value,
            _: &RequestContext,
        ) -> Result<serde_json::Value, AwsError> {
            Ok(serde_json::Value::Null)
        }
        async fn tick(&self) {
            panic!("intentional test panic from tick");
        }
    }

    #[tokio::test]
    async fn panicking_service_does_not_stop_other_services_ticking() {
        let counter = Arc::new(AtomicU64::new(0));
        let counting = Arc::new(CountingService {
            ticks: counter.clone(),
        }) as Arc<dyn ServiceHandler>;
        let panicky = Arc::new(PanickingService) as Arc<dyn ServiceHandler>;

        let mut services: HashMap<String, Arc<dyn ServiceHandler>> = HashMap::new();
        services.insert("counting".to_string(), counting);
        services.insert("panicky".to_string(), panicky);

        let mut state = AppState::new("us-east-1".to_string(), "000000000000".to_string());
        state.services = Arc::new(services);

        let handle = spawn_tick_loop(state, Duration::from_millis(30));
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();

        let count = counter.load(Ordering::SeqCst);
        assert!(
            count >= 3,
            "counting service should have continued ticking despite sibling panic; got {count}"
        );
    }

    /// Service whose `tick` sleeps past the per-handler deadline.
    /// The loop should time it out and keep going.
    struct SlowService {
        ticks: Arc<AtomicU64>,
    }

    #[async_trait::async_trait]
    impl ServiceHandler for SlowService {
        fn service_name(&self) -> &str {
            "slow"
        }
        fn protocol(&self) -> Protocol {
            Protocol::AwsJson1_1
        }
        async fn handle(
            &self,
            _: &str,
            _: serde_json::Value,
            _: &RequestContext,
        ) -> Result<serde_json::Value, AwsError> {
            Ok(serde_json::Value::Null)
        }
        async fn tick(&self) {
            self.ticks.fetch_add(1, Ordering::SeqCst);
            // Far exceed the 50ms per-handler deadline.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// Stub principal lookup that returns a fixed secret for every
    /// access key. Used by the presigned-URL verification test to
    /// drive the request through the SignatureMismatch path.
    struct FixedSecretLookup {
        secret: String,
    }

    impl crate::authz::PrincipalLookup for FixedSecretLookup {
        fn resolve_access_key(&self, _: &str) -> Option<crate::authz::ResolvedPrincipal> {
            None
        }
        fn resolve_secret(&self, _: &str) -> Option<String> {
            Some(self.secret.clone())
        }
    }

    #[test]
    fn presigned_url_tampering_surfaces_403_forbidden() {
        use axum::http::HeaderValue;

        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let mut state = AppState::new("us-east-1".to_string(), "000000000000".to_string());
        let mut authz = crate::authz::AuthzEngine::new(false);
        authz.principal_lookup = Arc::new(FixedSecretLookup {
            secret: secret.to_string(),
        });
        state.authz = Arc::new(authz);

        // Hand-crafted presigned URL with a deliberately wrong
        // signature. The verifier will recompute the expected signature
        // against `secret`, get a different value, and reject.
        let raw_query = "X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=AKID%2F20260524%2Fus-east-1%2Fs3%2Faws4_request\
&X-Amz-Date=20260524T120000Z\
&X-Amz-Expires=900\
&X-Amz-SignedHeaders=host\
&X-Amz-Signature=00000000000000000000000000000000\
00000000000000000000000000000000";
        let uri: Uri = format!("/bucket/key?{raw_query}").parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("s3.amazonaws.com"));

        let err = verify_presigned_for_request(&state, &headers, &Method::GET, &uri, "AKID")
            .expect_err("tampered presigned URL must be rejected");
        assert_eq!(err.code, "SignatureDoesNotMatch");
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "tampered presigned URL must surface as HTTP 403 (real AWS behaviour)"
        );
    }

    #[tokio::test]
    async fn slow_service_is_timed_out_so_loop_keeps_running() {
        let slow_ticks = Arc::new(AtomicU64::new(0));
        let fast_ticks = Arc::new(AtomicU64::new(0));
        let slow = Arc::new(SlowService {
            ticks: slow_ticks.clone(),
        }) as Arc<dyn ServiceHandler>;
        let fast = Arc::new(CountingService {
            ticks: fast_ticks.clone(),
        }) as Arc<dyn ServiceHandler>;

        let mut services: HashMap<String, Arc<dyn ServiceHandler>> = HashMap::new();
        services.insert("slow".to_string(), slow);
        services.insert("fast".to_string(), fast);

        let mut state = AppState::new("us-east-1".to_string(), "000000000000".to_string());
        state.services = Arc::new(services);

        let handle = spawn_tick_loop(state, Duration::from_millis(80));
        tokio::time::sleep(Duration::from_millis(400)).await;
        handle.abort();

        // Slow service starts each tick (increments before sleep) but
        // is cut off; fast service should accumulate multiple ticks
        // because the loop is not stuck waiting.
        let fast = fast_ticks.load(Ordering::SeqCst);
        assert!(
            fast >= 3,
            "fast service should keep ticking past the slow one; got {fast}"
        );
    }
}
