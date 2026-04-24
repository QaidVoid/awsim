use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::SecretsState;

pub struct SecretsManagerResourcePolicyLookup {
    store: AccountRegionStore<SecretsState>,
}

impl SecretsManagerResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<SecretsState>) -> Self {
        Self { store }
    }
}

fn extract_secret_key(arn: &str) -> Option<String> {
    let rest = arn.strip_prefix("arn:aws:secretsmanager:")?;
    let parts: Vec<&str> = rest.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let resource = parts[2];
    let after_secret = resource.strip_prefix("secret:")?;
    Some(after_secret.to_string())
}

impl ResourcePolicyLookup for SecretsManagerResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        for (_, state) in self.store.iter_all() {
            for entry in state.secrets.iter() {
                if entry.value().arn == resource_arn {
                    let name = entry.key();
                    if let Some(raw) = state.resource_policies.get(name) {
                        return awsim_iam_policy::parse(raw.value()).ok();
                    }
                    return None;
                }
            }
            if let Some(suffixed) = extract_secret_key(resource_arn) {
                let bare_name = suffixed
                    .rsplit_once('-')
                    .map(|(n, _)| n)
                    .unwrap_or(&suffixed);
                if let Some(raw) = state.resource_policies.get(bare_name) {
                    return awsim_iam_policy::parse(raw.value()).ok();
                }
            }
        }
        None
    }
}
