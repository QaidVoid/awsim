use dashmap::DashMap;
use std::collections::HashMap;

/// ELB v2 state — per account+region.
#[derive(Debug, Default)]
pub struct ElbState {
    pub load_balancers: DashMap<String, LoadBalancer>,
    pub target_groups: DashMap<String, TargetGroup>,
    pub listeners: DashMap<String, Listener>,
    pub rules: DashMap<String, Rule>,
    /// LB ARN → stored attributes (key-value pairs)
    pub lb_attributes: DashMap<String, Vec<AttributeKeyValue>>,
    /// Target group ARN → stored attributes
    pub tg_attributes: DashMap<String, Vec<AttributeKeyValue>>,
    /// Listener ARN → certificates
    pub listener_certificates: DashMap<String, Vec<Certificate>>,
}

/// A generic key-value attribute pair used for LB and TG attributes.
#[derive(Debug, Clone)]
pub struct AttributeKeyValue {
    pub key: String,
    pub value: String,
}

/// A listener certificate.
#[derive(Debug, Clone)]
pub struct Certificate {
    pub certificate_arn: String,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct LoadBalancer {
    pub arn: String,
    pub name: String,
    pub dns_name: String,
    pub lb_type: String,
    pub scheme: String,
    pub state: String,
    pub subnets: Vec<String>,
    pub security_groups: Vec<String>,
    pub tags: HashMap<String, String>,
    pub created_at: String,
    pub vpc_id: String,
}

#[derive(Debug, Clone)]
pub struct TargetGroup {
    pub arn: String,
    pub name: String,
    pub protocol: String,
    pub port: u16,
    pub vpc_id: String,
    pub target_type: String,
    pub targets: Vec<Target>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct Listener {
    pub arn: String,
    pub load_balancer_arn: String,
    pub port: u16,
    pub protocol: String,
    pub default_actions: Vec<ListenerAction>,
}

#[derive(Debug, Clone)]
pub struct ListenerAction {
    pub action_type: String,
    pub target_group_arn: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub arn: String,
    pub listener_arn: String,
    pub priority: String,
    pub conditions: Vec<serde_json::Value>,
    pub actions: Vec<ListenerAction>,
    pub is_default: bool,
}
