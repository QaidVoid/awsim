use dashmap::DashMap;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Instance {
    pub instance_arn: String,
    pub identity_store_id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PermissionSet {
    pub arn: String,
    pub name: String,
    pub description: String,
    pub session_duration: String,
    pub relay_state: String,
    pub created_at: u64,
    pub managed_policies: Vec<String>,
    pub inline_policy: String,
}

#[derive(Debug, Clone)]
pub struct AccountAssignment {
    pub id: String,
    pub instance_arn: String,
    pub permission_set_arn: String,
    pub account_id: String,
    pub principal_id: String,
    pub principal_type: String,
    pub status: String,
    pub target_type: String,
    pub target_id: String,
    pub requested_at: u64,
    pub request_type: String,
}

#[derive(Debug, Default)]
pub struct SsoAdminState {
    pub instances: DashMap<String, Instance>,
    pub permission_sets: DashMap<String, PermissionSet>,
    pub account_assignments: DashMap<String, AccountAssignment>,
    pub inline_policies: DashMap<String, Value>,
}

impl SsoAdminState {
    pub fn ensure_default_instance(
        &self,
        partition: &str,
        account_id: &str,
        _region: &str,
    ) -> Instance {
        let arn = format!("arn:{partition}:sso:::instance/ssoins-{account_id}");
        if let Some(i) = self.instances.get(&arn) {
            return i.clone();
        }
        let instance = Instance {
            instance_arn: arn.clone(),
            identity_store_id: format!("d-{account_id}"),
            name: "default".to_string(),
            status: "ACTIVE".to_string(),
        };
        self.instances.insert(arn, instance.clone());
        instance
    }
}
