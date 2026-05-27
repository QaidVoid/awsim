use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use dashmap::DashMap;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SentEmail {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub reply_to: Vec<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub raw: Option<String>,
    pub sent_at: u64,
    pub configuration_set_name: Option<String>,
    /// EmailTags supplied by the caller. AWS persists them for event
    /// destination dimensions and bounce/complaint reports.
    pub tags: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EmailIdentity {
    pub identity: String,
    pub verified: bool,
    pub identity_type: String,
    pub created_at: u64,
    /// DKIM signing config. `AWS_SES` is the managed `EASY_DKIM` flow;
    /// `EXTERNAL` is BYODKIM where the caller supplies the private key.
    pub dkim_signing_attributes_origin: Option<String>,
    pub dkim_signing_enabled: bool,
    pub dkim_status: Option<String>,
    pub dkim_domain_signing_selector: Option<String>,
    /// Stored verbatim from BYODKIM input; never read back by GetEmailIdentity.
    pub dkim_domain_signing_private_key: Option<String>,
    pub dkim_next_signing_key_length: Option<String>,
    /// MAIL FROM configuration. AWS stores both the custom domain and
    /// the behavior to apply when the MX lookup fails.
    pub mail_from_domain: Option<String>,
    pub mail_from_behavior_on_mx_failure: Option<String>,
    /// Default configuration set attached to this identity. Used as a
    /// fallback when a SendEmail call doesn't name one explicitly.
    pub configuration_set_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EmailTemplate {
    pub name: String,
    pub subject: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigurationSet {
    pub name: String,
    pub tags: HashMap<String, String>,
    pub sending_enabled: bool,
    pub reputation_metrics_enabled: bool,
    /// Unix-epoch seconds when ReputationMetricsEnabled most recently
    /// toggled from false -> true. AWS returns this as `LastFreshStart`
    /// in GetConfigurationSet so monitoring tooling can scope reputation
    /// metrics to the current "fresh" window.
    pub reputation_last_fresh_start: Option<u64>,
    pub event_destinations: Vec<EventDestination>,
    /// DeliveryOptions.TlsPolicy. Either `REQUIRE` or `OPTIONAL`.
    /// SendEmail through this set rejects when the policy is REQUIRE and
    /// the caller signals that TLS is unavailable for the recipient.
    pub tls_policy: Option<String>,
    /// Dedicated IP pool used for sends through this configuration set.
    pub sending_pool_name: Option<String>,
    /// Per-configuration-set VDM options. AWS exposes
    /// `DashboardOptions.EngagementMetrics` and
    /// `GuardianOptions.OptimizedSharedDelivery`, each `ENABLED`/`DISABLED`.
    pub vdm_dashboard_engagement_metrics: Option<String>,
    pub vdm_guardian_optimized_shared_delivery: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EventDestination {
    pub name: String,
    pub enabled: bool,
    pub matching_event_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DedicatedIpPool {
    pub name: String,
    pub scaling_mode: String,
    pub ips: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ContactList {
    pub name: String,
    pub description: Option<String>,
    pub topics: Vec<serde_json::Value>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub email: String,
    pub list_name: String,
    pub topic_preferences: Vec<serde_json::Value>,
    pub unsubscribe_all: bool,
    pub attributes: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct SuppressedDestination {
    pub email: String,
    pub reason: String,
    pub last_update: u64,
}

#[derive(Debug, Clone)]
pub struct CustomVerificationTemplate {
    pub name: String,
    pub from: String,
    pub subject: String,
    pub content: String,
    pub success_url: String,
    pub failure_url: String,
}

#[derive(Debug, Default)]
pub struct SesState {
    pub identities: DashMap<String, EmailIdentity>,
    pub templates: DashMap<String, EmailTemplate>,
    pub configuration_sets: DashMap<String, ConfigurationSet>,
    pub dedicated_ip_pools: DashMap<String, DedicatedIpPool>,
    pub contact_lists: DashMap<String, ContactList>,
    pub contacts: DashMap<String, Contact>,
    pub suppressed_destinations: DashMap<String, SuppressedDestination>,
    pub custom_verification_templates: DashMap<String, CustomVerificationTemplate>,
    pub identity_policies: DashMap<String, HashMap<String, String>>,
    pub identity_tags: DashMap<String, HashMap<String, String>>,
    /// Account-level VDM attributes: stored verbatim and returned by
    /// `GetAccount`. AWS shape:
    /// `{ VdmEnabled: ENABLED|DISABLED, DashboardAttributes?: {...}, GuardianAttributes?: {...} }`.
    pub account_vdm_attributes: Mutex<Option<serde_json::Value>>,
    /// Account-level suppression attributes:
    /// `{ SuppressedReasons: [BOUNCE | COMPLAINT] }`.
    pub account_suppression_attributes: Mutex<Option<serde_json::Value>>,
    /// Outbound email persistence — populated by `SesService` on the
    /// first `get_state()` call so operations can write to it without
    /// holding a service handle.
    pub sqlite: OnceLock<Arc<crate::SqliteStore>>,
}

impl SesState {
    pub fn sqlite(&self) -> Option<&Arc<crate::SqliteStore>> {
        self.sqlite.get()
    }

    pub fn set_sqlite(&self, store: Arc<crate::SqliteStore>) {
        let _ = self.sqlite.set(store);
    }
}
