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
}

/// Serializable snapshot.
#[derive(Debug, Serialize, Deserialize)]
pub struct AcmStateSnapshot {
    pub certificates: Vec<Certificate>,
}

/// Per-account/region ACM state.
#[derive(Debug, Default)]
pub struct AcmState {
    /// CertificateArn → Certificate
    pub certificates: DashMap<String, Certificate>,
}

impl AcmState {
    pub fn to_snapshot(&self) -> AcmStateSnapshot {
        AcmStateSnapshot {
            certificates: self.certificates.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: AcmStateSnapshot) {
        for cert in snapshot.certificates {
            self.certificates.insert(cert.certificate_arn.clone(), cert);
        }
    }
}
