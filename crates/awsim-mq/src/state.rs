use awsim_core::idempotency::IdempotencyCache;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// MQ honors `CreatorRequestId` for 24 hours per AWS.
const CREATOR_REQUEST_ID_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Default)]
pub struct MqState {
    pub brokers: DashMap<String, Broker>,
    /// (broker_id, username) keyed.
    pub users: DashMap<String, BrokerUser>,
    pub configurations: DashMap<String, Configuration>,
    /// `CreatorRequestId` cache keyed by token. A replay within
    /// `CREATOR_REQUEST_ID_TTL` returns the cached response payload;
    /// the same token with a different request body surfaces
    /// `IdempotencyParameterMismatchException`. The cache lives per
    /// `(account_id, region)` because the surrounding `MqState`
    /// already is.
    pub creator_request_cache: IdempotencyCacheValue,
}

/// Type alias kept short so the field declaration stays readable.
pub type IdempotencyCacheValue = IdempotencyCache<serde_json::Value>;

/// MQ's TTL for `CreatorRequestId` replays. Re-exported so service
/// code that needs the constant doesn't reach into the module.
pub fn creator_request_id_ttl() -> Duration {
    CREATOR_REQUEST_ID_TTL
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Broker {
    pub broker_id: String,
    pub broker_arn: String,
    pub broker_name: String,
    pub broker_state: String,
    pub broker_instance_type: String,
    pub deployment_mode: String,
    pub engine_type: String,
    pub engine_version: String,
    pub auto_minor_version_upgrade: bool,
    pub publicly_accessible: bool,
    pub host_instance_type: String,
    pub created: f64,
    pub authentication_strategy: String,
    pub storage_type: String,
    pub security_groups: Vec<String>,
    pub subnet_ids: Vec<String>,
    pub tags: std::collections::HashMap<String, String>,
    /// `EncryptionOptions` echoed back on Describe. AWS accepts a
    /// `KmsKeyId` (any string with the documented ARN shape) plus a
    /// `UseAwsOwnedKey` boolean; we store the raw JSON so future
    /// fields round-trip without a struct migration.
    #[serde(default)]
    pub encryption_options: Option<serde_json::Value>,
    /// Logs config (`{ "General": bool, "Audit": bool }`). AWS only
    /// allows Audit on ActiveMQ.
    #[serde(default)]
    pub logs: Option<serde_json::Value>,
    /// `MaintenanceWindowStartTime` echoed on Describe.
    #[serde(default)]
    pub maintenance_window_start_time: Option<serde_json::Value>,
    /// LDAP config — required when `AuthenticationStrategy=LDAP`.
    #[serde(default)]
    pub ldap_server_metadata: Option<serde_json::Value>,
    /// Initial Configuration reference: `{ "Id": "c-...", "Revision": 1 }`.
    /// AWS pins a configuration revision at create time; later
    /// `UpdateBroker` calls bump it.
    #[serde(default)]
    pub configuration: Option<serde_json::Value>,
    /// Cross-region read-replica posture (`NONE` | `CRDR`). AWS only
    /// supports it on RabbitMQ today; we just echo the configured
    /// value.
    #[serde(default)]
    pub data_replication_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerUser {
    pub broker_id: String,
    pub username: String,
    pub console_access: bool,
    pub groups: Vec<String>,
    pub replication_user: bool,
    pub pending_change: Option<String>,
    /// SHA-256 of the user-supplied password. Stored only so we can
    /// validate password changes; DescribeUser must never surface it.
    #[serde(default)]
    pub password_hash: Option<String>,
    /// In-flight UpdateUser payload. Cleared once the broker "reboots"
    /// (the moment AWS applies the change). Stored verbatim as a
    /// JSON object with the same fields a Describe response uses,
    /// minus credentials.
    #[serde(default)]
    pub pending: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub configuration_id: String,
    pub configuration_arn: String,
    pub name: String,
    pub engine_type: String,
    pub engine_version: String,
    pub authentication_strategy: String,
    pub created: f64,
    pub latest_revision: u32,
    pub description: Option<String>,
    /// Per-revision history. Index `N-1` is revision `N`. AWS retains
    /// every revision indefinitely so callers can roll a broker back
    /// to an earlier config; we mirror that.
    #[serde(default)]
    pub revisions: Vec<ConfigurationRevision>,
}

/// A single revision of a `Configuration`. AWS stores the broker's
/// XML/JSON config bytes under `Data` (base64-encoded) plus the
/// optional `Description` for the revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationRevision {
    pub revision: u32,
    pub created: f64,
    pub description: Option<String>,
    /// Base64-encoded config payload from `UpdateConfiguration.Data`.
    /// Always set; revision 1 (created via `CreateConfiguration`) gets
    /// an empty string until the caller first `UpdateConfiguration`s.
    pub data: String,
}

pub fn user_key(broker_id: &str, username: &str) -> String {
    format!("{broker_id}|{username}")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MqSnapshot {
    pub brokers: Vec<Broker>,
    pub users: Vec<BrokerUser>,
    pub configurations: Vec<Configuration>,
}

impl MqState {
    pub fn to_snapshot(&self) -> MqSnapshot {
        MqSnapshot {
            brokers: self.brokers.iter().map(|e| e.value().clone()).collect(),
            users: self.users.iter().map(|e| e.value().clone()).collect(),
            configurations: self
                .configurations
                .iter()
                .map(|e| e.value().clone())
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: MqSnapshot) {
        self.brokers.clear();
        self.users.clear();
        self.configurations.clear();
        for b in snap.brokers {
            self.brokers.insert(b.broker_id.clone(), b);
        }
        for u in snap.users {
            self.users.insert(user_key(&u.broker_id, &u.username), u);
        }
        for c in snap.configurations {
            self.configurations.insert(c.configuration_id.clone(), c);
        }
    }
}
