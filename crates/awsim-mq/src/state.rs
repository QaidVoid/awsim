use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct MqState {
    pub brokers: DashMap<String, Broker>,
    /// (broker_id, username) keyed.
    pub users: DashMap<String, BrokerUser>,
    pub configurations: DashMap<String, Configuration>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerUser {
    pub broker_id: String,
    pub username: String,
    pub console_access: bool,
    pub groups: Vec<String>,
    pub replication_user: bool,
    pub pending_change: Option<String>,
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
