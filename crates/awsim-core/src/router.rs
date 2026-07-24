/// Default AWS partition used when no override is configured.
pub const DEFAULT_PARTITION: &str = "aws";

/// Context extracted from an incoming AWS API request.
///
/// Contains the account ID, region, service, and request metadata
/// needed by service handlers to process the request.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// AWS account ID (default: "000000000000" in bypass mode)
    pub account_id: String,

    /// AWS region (e.g., "us-east-1")
    pub region: String,

    /// AWS partition: `aws`, `aws-cn`, `aws-us-gov`, `aws-iso(-b)`. Used
    /// in every ARN this request emits.
    pub partition: String,

    /// Service name extracted from the request
    pub service: String,

    /// Access key ID (if present in Authorization header)
    pub access_key: Option<String>,

    /// Unique request ID for this API call
    pub request_id: String,

    /// HTTP method of the original request
    pub method: String,

    /// URI path of the original request
    pub uri: String,

    /// Internal event bus - present for requests routed through the gateway;
    /// `None` in unit tests or any context where no bus was configured.
    pub event_bus: Option<crate::events::EventBus>,

    /// Source IP of the caller, if recoverable from `X-Forwarded-For` (or
    /// any future axum-side wiring of `ConnectInfo`). Surfaced into
    /// `aws:SourceIp` for IAM condition evaluation.
    pub source_ip: Option<String>,

    /// Whether the original request reached us over TLS, recovered from
    /// `X-Forwarded-Proto` when present. Surfaced as `aws:SecureTransport`.
    pub is_secure: bool,

    /// True when the request originated inside the server (bootstrap
    /// setup, background tasks) rather than from an external HTTP call.
    /// Used to bypass guardrails that AWS-parity demands of external
    /// callers but that the server itself must be able to perform during
    /// startup. For example, real AWS forbids any IAM mutation against
    /// the root user, but AWSim's first-run setup must be able to
    /// CreateUser("root") to provision the account-owner record.
    pub internal_bypass: bool,

    /// Billable request units consumed by this call, in thousandths so
    /// fractional charges (DynamoDB's 0.5-RRU eventually consistent
    /// reads) stay exact. Service handlers accumulate into it at their
    /// capacity charge sites via [`add_request_units`](Self::add_request_units);
    /// the dispatcher surfaces the total to the billing meter through
    /// the `X-Awsim-Request-Units` response header. Shared via `Arc`
    /// so clones moved into blocking closures feed the same counter.
    pub request_units_milli: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            region: String::new(),
            partition: DEFAULT_PARTITION.to_string(),
            service: String::new(),
            access_key: None,
            request_id: String::new(),
            method: String::new(),
            uri: String::new(),
            event_bus: None,
            source_ip: None,
            is_secure: false,
            internal_bypass: false,
            request_units_milli: Default::default(),
        }
    }
}

impl RequestContext {
    pub fn new(service: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            account_id: "000000000000".to_string(),
            region: region.into(),
            partition: DEFAULT_PARTITION.to_string(),
            service: service.into(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: "/".to_string(),
            event_bus: None,
            source_ip: None,
            is_secure: false,
            internal_bypass: false,
            request_units_milli: Default::default(),
        }
    }

    /// Like [`new`] but with an explicit account id, used by background
    /// pollers that fan out across every (account, region) pair.
    pub fn new_with_account(
        service: impl Into<String>,
        region: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            region: region.into(),
            partition: DEFAULT_PARTITION.to_string(),
            service: service.into(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: "/".to_string(),
            event_bus: None,
            source_ip: None,
            is_secure: false,
            internal_bypass: false,
            request_units_milli: Default::default(),
        }
    }

    /// Builder variant for server-internal callers (bootstrap setup,
    /// background tasks). Identical to [`new_with_account`] except the
    /// resulting context has [`internal_bypass`](Self::internal_bypass)
    /// set to `true`, so guardrails that would reject the same call
    /// coming from an external HTTP client (notably the root-user
    /// protection in awsim-iam) will let it through.
    pub fn internal(
        service: impl Into<String>,
        region: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        let mut ctx = Self::new_with_account(service, region, account_id);
        ctx.internal_bypass = true;
        ctx
    }

    /// Record billable request units (e.g. DynamoDB RCU/WCU) consumed
    /// by this request. No-op for non-positive values.
    pub fn add_request_units(&self, units: f64) {
        if units > 0.0 {
            let milli = (units * 1000.0).round() as u64;
            self.request_units_milli
                .fetch_add(milli, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Total billable units accumulated so far; 0.0 for services that
    /// don't meter per-request units.
    pub fn request_units(&self) -> f64 {
        self.request_units_milli
            .load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0
    }

    /// Returns an ARN prefix for this partition, account, and region.
    ///
    /// e.g. `arn:aws:s3:us-east-1:000000000000`.
    pub fn arn_prefix(&self, service: &str) -> String {
        format!(
            "arn:{}:{}:{}:{}",
            self.partition, service, self.region, self.account_id
        )
    }
}
