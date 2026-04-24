use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::KmsState;

pub struct KmsResourcePolicyLookup {
    store: AccountRegionStore<KmsState>,
}

impl KmsResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<KmsState>) -> Self {
        Self { store }
    }
}

fn extract_key_id(arn: &str) -> Option<String> {
    if let Some(rest) = arn.strip_prefix("arn:aws:kms:") {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() == 3 {
            let resource = parts[2];
            if let Some(key_id) = resource.strip_prefix("key/") {
                return Some(key_id.to_string());
            }
        }
    }
    None
}

impl ResourcePolicyLookup for KmsResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        let key_id = extract_key_id(resource_arn)?;
        for (_, state) in self.store.iter_all() {
            if let Some(key) = state.keys.get(&key_id) {
                if let Some(raw) = key
                    .policies
                    .get("default")
                    .or_else(|| key.policies.values().next())
                {
                    return awsim_iam_policy::parse(raw).ok();
                }
            }
        }
        None
    }
}
