use awsim_core::idempotency::IdempotencyCache;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ServiceDiscoveryState {
    pub namespaces: DashMap<String, Namespace>,
    pub services: DashMap<String, ServiceEntry>,
    /// (service_id, instance_id) keyed.
    pub instances: DashMap<String, Instance>,
    /// operation_id → tracked async operation; the emulator treats every
    /// operation as immediately SUCCESS so callers can poll once and move on.
    pub operations: DashMap<String, Operation>,
    /// CreatorRequestId idempotency for `Create*` paths.
    /// The cached value is the full successful response, so a
    /// duplicate call returns byte-identical output without
    /// re-running the work.
    pub creator_request_cache: IdempotencyCache<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub id: String,
    pub arn: String,
    pub name: String,
    pub r#type: String, // DNS_PUBLIC | DNS_PRIVATE | HTTP
    pub description: Option<String>,
    pub service_count: u32,
    pub create_date: f64,
    pub creator_request_id: Option<String>,
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub id: String,
    pub arn: String,
    pub name: String,
    pub namespace_id: String,
    pub description: Option<String>,
    pub instance_count: u32,
    pub dns_config: Option<serde_json::Value>,
    pub health_check_config: Option<serde_json::Value>,
    pub health_check_custom_config: Option<serde_json::Value>,
    pub create_date: f64,
    pub creator_request_id: Option<String>,
    pub r#type: String, // DNS | HTTP
    /// Monotonic counter bumped on every Register/Deregister against
    /// this service. Returned as `InstancesRevision` by
    /// `DiscoverInstances` so callers can detect changes between polls
    /// without re-comparing the full instance set.
    #[serde(default)]
    pub instances_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub service_id: String,
    pub creator_request_id: Option<String>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub r#type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub create_date: f64,
    pub update_date: f64,
    pub targets: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDiscoverySnapshot {
    pub namespaces: Vec<Namespace>,
    pub services: Vec<ServiceEntry>,
    pub instances: Vec<Instance>,
    pub operations: Vec<Operation>,
}

impl ServiceDiscoveryState {
    pub fn to_snapshot(&self) -> ServiceDiscoverySnapshot {
        ServiceDiscoverySnapshot {
            namespaces: self.namespaces.iter().map(|e| e.value().clone()).collect(),
            services: self.services.iter().map(|e| e.value().clone()).collect(),
            instances: self.instances.iter().map(|e| e.value().clone()).collect(),
            operations: self.operations.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: ServiceDiscoverySnapshot) {
        self.namespaces.clear();
        self.services.clear();
        self.instances.clear();
        self.operations.clear();
        for n in snap.namespaces {
            self.namespaces.insert(n.id.clone(), n);
        }
        for s in snap.services {
            self.services.insert(s.id.clone(), s);
        }
        for i in snap.instances {
            let key = format!("{}:{}", i.service_id, i.id);
            self.instances.insert(key, i);
        }
        for o in snap.operations {
            self.operations.insert(o.id.clone(), o);
        }
    }
}
