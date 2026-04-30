use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SentEmail {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub raw: Option<String>,
    pub sent_at: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EmailIdentity {
    pub identity: String,
    pub verified: bool,
    pub identity_type: String,
    pub created_at: u64,
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
    pub event_destinations: Vec<EventDestination>,
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
