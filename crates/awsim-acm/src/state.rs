use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A DNS validation record for ACM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsValidationRecord {
    pub name: String,
    pub record_type: String,
    pub value: String,
}

/// A single ACM certificate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub certificate_arn: String,
    pub domain_name: String,
    pub subject_alternative_names: Vec<String>,
    pub status: String,
    pub validation_method: String,
    /// DNS validation records keyed by domain name.
    pub dns_validation_records: HashMap<String, DnsValidationRecord>,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    /// ARNs of resources that reference this certificate. Populated by
    /// downstream services (e.g. ELBv2, CloudFront) via the cross-service
    /// event sink. DeleteCertificate is rejected while non-empty.
    #[serde(default)]
    pub in_use_by: Vec<String>,
    /// `ENABLED` or `DISABLED`; AWS defaults to `ENABLED` when omitted
    /// and lets callers flip the value via UpdateCertificateOptions.
    #[serde(default = "default_ct_pref")]
    pub certificate_transparency_logging_preference: String,
    /// `AMAZON_ISSUED`, `IMPORTED`, or `PRIVATE`. Drives
    /// `RenewalEligibility` — imported certs are always
    /// `INELIGIBLE` because AWS has no key material to renew.
    #[serde(default = "default_cert_type")]
    pub certificate_type: String,
    /// Key algorithm picked at create/import time. Surfaced via
    /// `Describe` and filtered via `ListCertificates.Includes.keyTypes`.
    #[serde(default = "default_key_algorithm")]
    pub key_algorithm: String,
}

fn default_ct_pref() -> String {
    "ENABLED".to_string()
}

fn default_cert_type() -> String {
    "AMAZON_ISSUED".to_string()
}

fn default_key_algorithm() -> String {
    "RSA_2048".to_string()
}

/// Serializable snapshot.
#[derive(Debug, Serialize, Deserialize)]
pub struct AcmStateSnapshot {
    pub certificates: Vec<Certificate>,
}

/// Account-level ACM configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcmAccountConfig {
    pub expiry_events_configuration: Option<serde_json::Value>,
}

/// Per-account/region ACM state.
#[derive(Debug, Default)]
pub struct AcmState {
    /// CertificateArn → Certificate
    pub certificates: DashMap<String, Certificate>,
    /// Account-level configuration (stored at "default" key)
    pub account_config: DashMap<String, AcmAccountConfig>,
    /// RequestCertificate IdempotencyToken cache. AWS preserves the
    /// original response for 24 hours and rejects param mismatches with
    /// `IdempotentParameterMismatch`.
    pub request_idempotency: awsim_core::idempotency::IdempotencyCache<serde_json::Value>,
}

impl AcmState {
    pub fn to_snapshot(&self) -> AcmStateSnapshot {
        AcmStateSnapshot {
            certificates: self
                .certificates
                .iter()
                .map(|e| e.value().clone())
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: AcmStateSnapshot) {
        for cert in snapshot.certificates {
            self.certificates.insert(cert.certificate_arn.clone(), cert);
        }
    }
}
