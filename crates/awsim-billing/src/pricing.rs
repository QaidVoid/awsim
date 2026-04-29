use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level catalog: one `ServicePricing` per service, indexed by signing
/// name (e.g. `s3`, `lambda`, `dynamodb`).
#[derive(Debug, Default, Clone)]
pub struct PricingCatalog {
    services: HashMap<String, ServicePricing>,
}

impl PricingCatalog {
    /// Build the catalog from the JSON files embedded at compile time.
    pub fn embedded() -> Self {
        let mut services = HashMap::new();
        for raw in EMBEDDED_PRICING {
            match serde_json::from_str::<ServicePricing>(raw) {
                Ok(svc) => {
                    services.insert(svc.service.clone(), svc);
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to parse embedded pricing JSON");
                }
            }
        }
        Self { services }
    }

    pub fn get(&self, service: &str) -> Option<&ServicePricing> {
        self.services.get(service)
    }

    pub fn services(&self) -> impl Iterator<Item = (&String, &ServicePricing)> {
        self.services.iter()
    }

    pub fn len(&self) -> usize {
        self.services.len()
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePricing {
    /// Signing name, must match `RequestEvent.service` (e.g. "s3").
    pub service: String,
    /// Human-readable display name (e.g. "Amazon S3").
    pub display_name: String,
    /// Region the prices apply to (us-east-1 only at present).
    pub region: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Free-text source pointer (URL, page name) for traceability.
    #[serde(default)]
    pub source: Option<String>,
    /// Per-request dimensions. The first dimension whose `operations` list
    /// matches an event's operation name wins.
    #[serde(default)]
    pub request_dimensions: Vec<RequestDimension>,
    /// Fallback rate for any operation not matched by a dimension above.
    #[serde(default)]
    pub default_request_rate: Option<f64>,
    /// Cost per GB of outbound response payload.
    #[serde(default)]
    pub data_transfer_out_per_gb: Option<f64>,
    /// Cost per GB of inbound request payload — used for ingest-billed
    /// services (Firehose, CloudWatch Logs ingest, etc.) where AWS
    /// charges by data volume rather than per-request.
    #[serde(default)]
    pub data_ingest_per_gb: Option<f64>,
    /// Cost per GB stored per month — point-in-time billing for
    /// services like S3 / DynamoDB / Lambda function code. The meter
    /// samples the at-rest size periodically and accrues cost over
    /// elapsed time using this rate.
    #[serde(default)]
    pub storage_per_gb_month: Option<f64>,
    /// Cost per GB-second of compute time — Lambda's invocation
    /// duration model. The meter multiplies request duration_ms by an
    /// assumed memory size (128 MB default — AWSim doesn't carry the
    /// per-function memory through the request event yet) and applies
    /// this rate.
    #[serde(default)]
    pub compute_per_gb_second: Option<f64>,
}

fn default_currency() -> String {
    "USD".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDimension {
    /// Description shown in the bill row, e.g. "PUT/COPY/POST/LIST requests".
    pub description: String,
    /// Operation names this dimension applies to (case-sensitive,
    /// matched against `RequestEvent.operation`).
    #[serde(default)]
    pub operations: Vec<String>,
    /// USD per single request.
    pub price_per_request: f64,
}

impl ServicePricing {
    /// Look up the per-request rate for the given operation. Falls back
    /// to `default_request_rate` if no dimension matched, and `0.0` if
    /// neither is set.
    pub fn rate_for(&self, operation: &str) -> (Option<&RequestDimension>, f64) {
        for dim in &self.request_dimensions {
            if dim.operations.iter().any(|op| op == operation) {
                return (Some(dim), dim.price_per_request);
            }
        }
        (None, self.default_request_rate.unwrap_or(0.0))
    }
}

const EMBEDDED_PRICING: &[&str] = &[
    include_str!("../pricing/s3.json"),
    include_str!("../pricing/lambda.json"),
    include_str!("../pricing/dynamodb.json"),
    include_str!("../pricing/sqs.json"),
    include_str!("../pricing/sns.json"),
    include_str!("../pricing/kms.json"),
    include_str!("../pricing/secretsmanager.json"),
    include_str!("../pricing/events.json"),
    include_str!("../pricing/apigateway.json"),
    include_str!("../pricing/states.json"),
    include_str!("../pricing/ses.json"),
    include_str!("../pricing/monitoring.json"),
    include_str!("../pricing/route53.json"),
    include_str!("../pricing/kinesis.json"),
    include_str!("../pricing/cloudfront.json"),
    include_str!("../pricing/firehose.json"),
    include_str!("../pricing/logs.json"),
    include_str!("../pricing/cognito-idp.json"),
    include_str!("../pricing/cognito-identity.json"),
    include_str!("../pricing/ecr.json"),
];
