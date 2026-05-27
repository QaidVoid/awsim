use awsim_core::{AccountRegionStore, ResourcePolicyLookup, SecretLookup};
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

/// Cross-service helper: implements [`SecretLookup`] so other crates
/// (ECS repositoryCredentials, container `secrets[]`, etc.) can ask
/// "does this secret exist in (account, region)?" without taking a
/// direct dependency on awsim-secretsmanager's internals.
pub struct SecretsManagerSecretLookup {
    store: AccountRegionStore<SecretsState>,
}

impl SecretsManagerSecretLookup {
    pub fn new(store: AccountRegionStore<SecretsState>) -> Self {
        Self { store }
    }
}

impl SecretLookup for SecretsManagerSecretLookup {
    fn secret_exists(&self, secret_ref: &str, account: &str, region: &str) -> bool {
        let state = self.store.get(account, region);
        if state.secrets.contains_key(secret_ref) {
            return true;
        }
        // ARN form: arn:aws:secretsmanager:{region}:{account}:secret:{name}-{suffix}
        // The 6-char suffix is generated at CreateSecret time and isn't
        // part of the canonical name key, so strip it before lookup.
        if let Some(stored_key) = extract_secret_key(secret_ref) {
            if state.secrets.contains_key(&stored_key) {
                return true;
            }
            let bare = stored_key
                .rsplit_once('-')
                .map(|(n, _)| n.to_string())
                .unwrap_or(stored_key);
            if state.secrets.contains_key(&bare) {
                return true;
            }
        }
        // Last resort: scan secrets for an exact ARN match.
        state.secrets.iter().any(|e| e.value().arn == secret_ref)
    }
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
