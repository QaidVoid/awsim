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

    /// Billable read/write request units consumed by this call.
    /// Service handlers accumulate into it at their capacity charge
    /// sites; the dispatcher surfaces the totals to the billing meter
    /// through the `X-Awsim-Read-Units` / `X-Awsim-Write-Units`
    /// response headers. Cloning shares the underlying counters, so
    /// clones moved into blocking closures feed the same tallies.
    pub request_units: RequestUnits,

    /// Authority (`host` or `host:port`) that this caller can reach AWSim
    /// on, used to build resource URLs the caller will subsequently use:
    /// SQS `QueueUrl`, API Gateway endpoints, AppSync GraphQL URLs,
    /// Lambda function URLs.
    ///
    /// Resolved by the gateway with the precedence: explicit operator
    /// override, then the request's `Host` header, then the listen
    /// address. `None` in unit tests and server-internal contexts, where
    /// [`base_url`](Self::base_url) falls back to the default endpoint.
    pub endpoint_authority: Option<String>,
}

/// Shared read/write billable-unit tallies for one request, stored in
/// thousandths so fractional charges (DynamoDB's 0.5-RRU eventually
/// consistent reads) stay exact. Read and write are separate axes
/// because AWS prices them differently and a single call (PartiQL
/// transactions) can consume both.
#[derive(Debug, Clone, Default)]
pub struct RequestUnits {
    read_milli: std::sync::Arc<std::sync::atomic::AtomicU64>,
    write_milli: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl RequestUnits {
    /// Record billable read units (e.g. DynamoDB RCU). No-op for
    /// non-positive values.
    pub fn add_read(&self, units: f64) {
        Self::add(&self.read_milli, units);
    }

    /// Record billable write units (e.g. DynamoDB WCU). No-op for
    /// non-positive values.
    pub fn add_write(&self, units: f64) {
        Self::add(&self.write_milli, units);
    }

    fn add(cell: &std::sync::atomic::AtomicU64, units: f64) {
        if units > 0.0 {
            cell.fetch_add(
                (units * 1000.0).round() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }

    /// Total billable read units accumulated so far.
    pub fn read(&self) -> f64 {
        self.read_milli.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0
    }

    /// Total billable write units accumulated so far.
    pub fn write(&self) -> f64 {
        self.write_milli.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0
    }
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
            request_units: Default::default(),
            endpoint_authority: None,
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
            request_units: Default::default(),
            endpoint_authority: None,
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
            request_units: Default::default(),
            endpoint_authority: None,
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

    /// Returns an ARN prefix for this partition, account, and region.
    ///
    /// e.g. `arn:aws:s3:us-east-1:000000000000`.
    pub fn arn_prefix(&self, service: &str) -> String {
        format!(
            "arn:{}:{}:{}:{}",
            self.partition, service, self.region, self.account_id
        )
    }

    /// Scheme this caller reached AWSim on.
    pub fn scheme(&self) -> &'static str {
        if self.is_secure { "https" } else { "http" }
    }

    /// Authority (`host` or `host:port`) to put in returned resource URLs.
    ///
    /// Falls back to the historical default when unresolved, which keeps
    /// unit tests and server-internal contexts working unchanged.
    pub fn authority(&self) -> &str {
        self.endpoint_authority
            .as_deref()
            .unwrap_or(DEFAULT_ENDPOINT_AUTHORITY)
    }

    /// Base URL a caller can reach AWSim on, e.g. `http://localhost:4566`.
    ///
    /// Use this rather than hardcoding an endpoint: a resource URL built
    /// from a literal is wrong the moment AWSim listens on another port,
    /// runs under Testcontainers (which maps to a random host port), or is
    /// addressed by container name from a sibling container.
    pub fn base_url(&self) -> String {
        format!("{}://{}", self.scheme(), self.authority())
    }

    /// Base URL in the AWS service-subdomain form, e.g.
    /// `http://sqs.us-east-1.localhost:4566`, mirroring
    /// `sqs.us-east-1.amazonaws.com`. Some clients parse the region back
    /// out of a queue URL, so the shape is preserved where it can be.
    ///
    /// Falls back to the plain [`base_url`](Self::base_url) when the
    /// resolved host cannot carry a subdomain prefix, which is the case
    /// for an IP address (`sqs.us-east-1.127.0.0.1` does not resolve).
    pub fn service_base_url(&self, service: &str) -> String {
        let authority = self.authority();
        let host = authority.split(':').next().unwrap_or(authority);
        if host_accepts_subdomain(host) {
            format!("{}://{service}.{}.{authority}", self.scheme(), self.region)
        } else {
            self.base_url()
        }
    }
}

/// Endpoint used when nothing better is known: unit tests, background
/// tasks, and any context the gateway did not populate.
pub const DEFAULT_ENDPOINT_AUTHORITY: &str = "localhost:4566";

/// Whether `host` can carry a `service.region.` prefix and still resolve.
/// IPv4 and IPv6 literals cannot.
fn host_accepts_subdomain(host: &str) -> bool {
    if host.is_empty() || host.starts_with('[') || host.contains(':') {
        return false;
    }
    !host
        .split('.')
        .all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_digit()))
}
