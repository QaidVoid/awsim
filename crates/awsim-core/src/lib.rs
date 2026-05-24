pub mod arn;
pub mod auth;
pub mod authz;
pub mod bearer_token;
pub mod body;
pub mod body_store;
pub mod error;
pub mod events;
pub mod gateway;
pub mod idempotency;
pub mod lifecycle;
pub mod pagination;
pub mod persistence;
pub mod protocol;
pub mod request_detail;
pub mod request_event;
pub mod router;
pub mod state;
pub mod tags;
pub mod tick;

pub use authz::{
    AuthzEngine, GrantLookup, NoopPrincipalLookup, PrincipalLookup, ResolvedPrincipal,
    ResourcePolicyLookup, ScpLookup,
};
// `HandlerByteStream` and `HandlerResult` are defined further down in
// this file; re-exported here for crate consumers.
pub use arn::Arn;
pub use body::Body;
pub use body_store::{BlobInventory, BodyStore};
pub use error::AwsError;
pub use events::{EventBus, InternalEvent};
pub use gateway::{AppState, BodyStoreHandle};
pub use pagination::{Page, cap_max_results, decode_token, encode_token, paginate};
pub use persistence::PersistenceManager;
pub use protocol::{Protocol, RouteDefinition};
pub use request_detail::{
    CapturedBody, CapturedHeader, DEFAULT_BODY_CAP, DEFAULT_RING_CAPACITY, RequestDetail,
    RequestDetailStore, capture_body, capture_headers,
};
pub use request_event::{RequestEvent, RequestEventBus};
pub use router::{DEFAULT_PARTITION, RequestContext};
pub use state::{AccountRegionStore, Snapshottable};
pub use tick::WorkerPool;

use bytes::Bytes;
use futures::stream::BoxStream;
use serde_json::Value;

/// Boxed byte stream a handler may return when it wants to drive an
/// HTTP response chunk-by-chunk (e.g. Bedrock's event-stream APIs)
/// instead of buffering the whole response into a single `Value`.
pub type HandlerByteStream = BoxStream<'static, Result<Bytes, AwsError>>;

/// What `ServiceHandler::handle_streaming` returns. Most operations
/// produce a single JSON `Value` (the existing path); a small set —
/// notably Bedrock's `ConverseStream` and
/// `InvokeModelWithResponseStream` — produce a continuous stream of
/// already-encoded body bytes plus a content-type the gateway puts
/// straight on the wire.
pub enum HandlerResult {
    /// Conventional single-shot response. The gateway runs it
    /// through the normal protocol serializer.
    Json(Value),
    /// Streamed binary body. The gateway sends it via axum's
    /// chunked-transfer body so the client sees bytes as they're
    /// produced — no buffering on our side.
    Streaming {
        body: HandlerByteStream,
        content_type: &'static str,
    },
}

impl From<Value> for HandlerResult {
    fn from(v: Value) -> Self {
        HandlerResult::Json(v)
    }
}

/// Trait that every AWS service crate must implement.
///
/// Each service (S3, SQS, DynamoDB, etc.) implements this trait in its own crate.
/// The main `awsim` binary registers all service handlers with the gateway router.
#[async_trait::async_trait]
pub trait ServiceHandler: Send + Sync {
    /// The AWS service name (e.g., "s3", "sqs", "dynamodb").
    fn service_name(&self) -> &str;

    /// The signing name used in SigV4 Authorization headers.
    /// Usually the same as service_name, but not always.
    fn signing_name(&self) -> &str {
        self.service_name()
    }

    /// The primary protocol this service uses.
    fn protocol(&self) -> Protocol;

    /// Route definitions for REST-protocol services.
    /// Not needed for RPC-style protocols (awsJson, awsQuery).
    fn routes(&self) -> Vec<RouteDefinition> {
        Vec::new()
    }

    /// Handle an AWS API operation.
    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError>;

    /// Streaming-aware variant. The default delegates to `handle` and
    /// wraps the JSON result so existing services don't need to do
    /// anything; services that genuinely stream (Bedrock data plane)
    /// override this and return `HandlerResult::Streaming`.
    async fn handle_streaming(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<HandlerResult, AwsError> {
        self.handle(operation, input, ctx)
            .await
            .map(HandlerResult::from)
    }

    /// Serialize the service's state to bytes for persistence.
    ///
    /// Return `None` if this service does not support snapshots.
    fn snapshot(&self) -> Option<Vec<u8>> {
        None
    }

    /// Restore the service's state from a previous snapshot.
    ///
    /// The default implementation is a no-op and always succeeds.
    fn restore(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// Called after every service has been restored from its
    /// snapshot, before the gateway begins serving traffic.
    ///
    /// Use this to re-arm timers, restart event-source-mapping
    /// pollers, and re-register tick-driven workers that don't
    /// persist as data (because they are derivable from the
    /// already-restored state). The default implementation is a
    /// no-op so services that don't have background work pay
    /// nothing.
    ///
    /// Two ordering guarantees the gateway makes:
    /// - Every service's [`Self::restore`] completes before any
    ///   service's `rehydrate` is invoked, so cross-service
    ///   wiring (Lambda event source mappings reading SQS state,
    ///   say) sees the fully restored peer.
    /// - No request is dispatched until every service's
    ///   `rehydrate` returns.
    fn rehydrate(&self) -> Result<(), String> {
        Ok(())
    }

    fn iam_action(&self, _operation: &str) -> Option<String> {
        None
    }

    fn iam_resource(
        &self,
        _operation: &str,
        _input: &serde_json::Value,
        _ctx: &router::RequestContext,
    ) -> Option<String> {
        None
    }

    /// Periodic tick. The gateway spawns a single 1-second loop that
    /// calls `tick` on every registered service after the server is up.
    /// Use this hook for time-driven behavior that doesn't fit into the
    /// request path: SQS visibility-timeout reclamation, DynamoDB TTL
    /// expiry, Lambda event-source-mapping polling, S3 lifecycle
    /// transitions, EventBridge schedule firing, SecretsManager
    /// rotation, etc.
    ///
    /// **Contract:**
    /// - `tick` must be idempotent — it may be called repeatedly, and
    ///   missing a tick must not lose state. Use absolute deadlines
    ///   (`Instant`/`SystemTime`) rather than per-call deltas.
    /// - `tick` must return quickly (target <10 ms). Slow work
    ///   (HTTP fan-out, subprocess invocation, large iterations)
    ///   should be enqueued onto an internal worker the service spawns
    ///   from elsewhere — `tick` enqueues, doesn't block.
    /// - The default implementation is a no-op so existing services
    ///   don't need to opt in.
    async fn tick(&self) {}
}
