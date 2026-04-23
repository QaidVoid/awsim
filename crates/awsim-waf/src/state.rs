use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A WAF WebACL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAcl {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub arn: String,
    pub default_action: Value,
    pub rules: Vec<Value>,
    pub visibility_config: Value,
    pub lock_token: String,
    pub created_at: u64,
}

/// A WAF IP Set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpSet {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub arn: String,
    pub ip_address_version: String,
    pub addresses: Vec<String>,
    pub lock_token: String,
    pub created_at: u64,
}

/// A WAF Rule Group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub arn: String,
    pub capacity: u64,
    pub rules: Vec<Value>,
    pub lock_token: String,
    pub created_at: u64,
}

/// A WAF Logging Configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub resource_arn: String,
    pub log_destination_configs: Vec<String>,
    pub redacted_fields: Vec<Value>,
    pub managed_by_firewall_manager: bool,
    pub logging_filter: Option<Value>,
}

/// Serializable snapshot.
#[derive(Debug, Serialize, Deserialize)]
pub struct WafStateSnapshot {
    pub web_acls: Vec<WebAcl>,
    pub ip_sets: Vec<IpSet>,
    pub rule_groups: Vec<RuleGroup>,
    #[serde(default)]
    pub logging_configs: Vec<LoggingConfig>,
    #[serde(default)]
    pub web_acl_associations: Vec<(String, String)>,
}

/// Per-account/region WAF state.
#[derive(Debug, Default)]
pub struct WafState {
    /// "{scope}:{name}" → WebAcl
    pub web_acls: DashMap<String, WebAcl>,
    /// "{scope}:{name}" → IpSet
    pub ip_sets: DashMap<String, IpSet>,
    /// "{scope}:{name}" → RuleGroup
    pub rule_groups: DashMap<String, RuleGroup>,
    /// resource_arn → LoggingConfig
    pub logging_configs: DashMap<String, LoggingConfig>,
    /// resource_arn → web_acl_arn
    pub web_acl_associations: DashMap<String, String>,
}

impl WafState {
    pub fn to_snapshot(&self) -> WafStateSnapshot {
        WafStateSnapshot {
            web_acls: self.web_acls.iter().map(|e| e.value().clone()).collect(),
            ip_sets: self.ip_sets.iter().map(|e| e.value().clone()).collect(),
            rule_groups: self.rule_groups.iter().map(|e| e.value().clone()).collect(),
            logging_configs: self
                .logging_configs
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            web_acl_associations: self
                .web_acl_associations
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: WafStateSnapshot) {
        for acl in snapshot.web_acls {
            let key = format!("{}:{}", acl.scope, acl.name);
            self.web_acls.insert(key, acl);
        }
        for ip_set in snapshot.ip_sets {
            let key = format!("{}:{}", ip_set.scope, ip_set.name);
            self.ip_sets.insert(key, ip_set);
        }
        for rg in snapshot.rule_groups {
            let key = format!("{}:{}", rg.scope, rg.name);
            self.rule_groups.insert(key, rg);
        }
        for cfg in snapshot.logging_configs {
            self.logging_configs.insert(cfg.resource_arn.clone(), cfg);
        }
        for (resource_arn, web_acl_arn) in snapshot.web_acl_associations {
            self.web_acl_associations.insert(resource_arn, web_acl_arn);
        }
    }
}
