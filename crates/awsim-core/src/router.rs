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

    /// Internal event bus — present for requests routed through the gateway;
    /// `None` in unit tests or any context where no bus was configured.
    pub event_bus: Option<crate::events::EventBus>,
}

impl RequestContext {
    pub fn new(service: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            account_id: "000000000000".to_string(),
            region: region.into(),
            service: service.into(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: "/".to_string(),
            event_bus: None,
        }
    }

    /// Like [`new`] but with an explicit account id — used by background
    /// pollers that fan out across every (account, region) pair.
    pub fn new_with_account(
        service: impl Into<String>,
        region: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            region: region.into(),
            service: service.into(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: "/".to_string(),
            event_bus: None,
        }
    }

    /// Returns an ARN prefix for this account and region.
    /// e.g., "arn:aws:s3:us-east-1:000000000000"
    pub fn arn_prefix(&self, service: &str) -> String {
        format!("arn:aws:{}:{}:{}", service, self.region, self.account_id)
    }
}
