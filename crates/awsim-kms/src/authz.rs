use awsim_core::{AccountRegionStore, GrantLookup, ResourcePolicyLookup};
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

/// Bridge for KMS grants into the AuthzEngine. A grant on a key authorizes
/// the listed `Operations` for the `GranteePrincipal`, in addition to any
/// permissions the principal already has via identity / key policy.
pub struct KmsGrantLookup {
    store: AccountRegionStore<KmsState>,
}

impl KmsGrantLookup {
    pub fn new(store: AccountRegionStore<KmsState>) -> Self {
        Self { store }
    }
}

impl GrantLookup for KmsGrantLookup {
    fn allows(&self, principal_arn: &str, action: &str, resource_arn: &str) -> bool {
        // Action arrives as `kms:Decrypt`; grants store the bare op name.
        let op = action.strip_prefix("kms:").unwrap_or(action);
        let Some(target_key_id) = extract_key_id(resource_arn) else {
            return false;
        };
        for (_, state) in self.store.iter_all() {
            for entry in state.grants.iter() {
                let grant = entry.value();
                if grant.key_id != target_key_id {
                    continue;
                }
                if grant.grantee_principal != principal_arn {
                    continue;
                }
                if grant.operations.iter().any(|o| o == op) {
                    return true;
                }
            }
        }
        false
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
            if let Some(key) = state.keys.get(&key_id)
                && let Some(raw) = key
                    .policies
                    .get("default")
                    .or_else(|| key.policies.values().next())
            {
                return awsim_iam_policy::parse(raw).ok();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use awsim_iam_policy::{AuthzRequest, Decision, EvalContext, evaluate};

    use super::*;
    use crate::state::{KmsKey, KmsState};

    fn make_key_with_policy(key_id: &str, policy: &str) -> KmsKey {
        let mut policies = HashMap::new();
        policies.insert("default".to_string(), policy.to_string());
        KmsKey {
            key_id: key_id.to_string(),
            arn: format!("arn:aws:kms:us-east-1:000000000000:key/{key_id}"),
            description: "test".into(),
            key_state: "Enabled".into(),
            key_spec: "SYMMETRIC_DEFAULT".into(),
            key_usage: "ENCRYPT_DECRYPT".into(),
            creation_date: 0.0,
            secret: vec![0; 32],
            deletion_date: None,
            rotation_enabled: false,
            policies,
            tags: Default::default(),
            key_material_imported: false,
            origin: "AWS_KMS".into(),
        }
    }

    fn populate_store(policy: &str) -> (AccountRegionStore<KmsState>, String) {
        let store: AccountRegionStore<KmsState> = AccountRegionStore::new();
        let key_id = "11111111-2222-3333-4444-555555555555".to_string();
        let arn = format!("arn:aws:kms:us-east-1:000000000000:key/{key_id}");
        let state = store.get("000000000000", "us-east-1");
        state
            .keys
            .insert(key_id.clone(), make_key_with_policy(&key_id, policy));
        (store, arn)
    }

    #[test]
    fn lookup_returns_none_for_unknown_key() {
        let (store, _arn) = populate_store(
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"kms:*","Resource":"*"}]}"#,
        );
        let lookup = KmsResourcePolicyLookup::new(store);
        assert!(
            lookup
                .lookup("arn:aws:kms:us-east-1:000000000000:key/missing")
                .is_none()
        );
    }

    #[test]
    fn key_policy_is_consulted_during_authz_evaluation() {
        // Resource policy explicitly denies decrypts by anyone — expectation
        // is that even an identity policy that allows kms:Decrypt is
        // overridden by the explicit deny on the key itself.
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [
                {"Effect": "Allow", "Principal": "*", "Action": "kms:*", "Resource": "*"},
                {"Effect": "Deny", "Principal": "*", "Action": "kms:Decrypt", "Resource": "*"}
            ]
        }"#;
        let (store, arn) = populate_store(policy);
        let lookup = KmsResourcePolicyLookup::new(store);
        let resource_policy = lookup.lookup(&arn).expect("policy parses");

        let identity = awsim_iam_policy::parse(
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"kms:*","Resource":"*"}]}"#,
        )
        .unwrap();

        let context = HashMap::new();
        let req = AuthzRequest {
            principal_arn: "arn:aws:iam::000000000000:user/alice",
            principal_account: "000000000000",
            action: "kms:Decrypt",
            resource_arn: &arn,
            context: &context,
        };
        let scps: Vec<_> = Vec::new();
        let eval_ctx = EvalContext {
            identity_policies: &[identity],
            permissions_boundary: None,
            resource_policy: Some(&resource_policy),
            scps: &scps,
            session_policy: None,
        };
        assert!(matches!(evaluate(&req, &eval_ctx), Decision::ExplicitDeny));
    }

    #[test]
    fn grant_lookup_matches_principal_action_and_key() {
        use crate::state::KmsGrant;

        let store: AccountRegionStore<KmsState> = AccountRegionStore::new();
        let key_id = "11111111-2222-3333-4444-555555555555".to_string();
        let arn = format!("arn:aws:kms:us-east-1:000000000000:key/{key_id}");
        let state = store.get("000000000000", "us-east-1");
        state.keys.insert(
            key_id.clone(),
            make_key_with_policy(&key_id, r#"{"Version":"2012-10-17","Statement":[]}"#),
        );
        state.grants.insert(
            "g1".into(),
            KmsGrant {
                grant_id: "g1".into(),
                grant_token: "tok".into(),
                key_id: key_id.clone(),
                name: None,
                grantee_principal: "arn:aws:iam::000000000000:user/alice".into(),
                operations: vec!["Decrypt".into(), "DescribeKey".into()],
                token_created_at: 0,
            },
        );
        let lookup = KmsGrantLookup::new(store);

        // Matching principal + listed operation + correct key.
        assert!(lookup.allows("arn:aws:iam::000000000000:user/alice", "kms:Decrypt", &arn,));
        // Same principal but operation not in the grant.
        assert!(!lookup.allows("arn:aws:iam::000000000000:user/alice", "kms:Encrypt", &arn,));
        // Right operation, wrong principal.
        assert!(!lookup.allows("arn:aws:iam::000000000000:user/eve", "kms:Decrypt", &arn,));
        // Right grant but for a different key resource.
        let other_arn = "arn:aws:kms:us-east-1:000000000000:key/different".to_string();
        assert!(!lookup.allows(
            "arn:aws:iam::000000000000:user/alice",
            "kms:Decrypt",
            &other_arn,
        ));
    }

    #[test]
    fn allow_only_when_both_identity_and_key_policy_permit() {
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam::000000000000:user/alice"},
                "Action": "kms:Encrypt",
                "Resource": "*"
            }]
        }"#;
        let (store, arn) = populate_store(policy);
        let lookup = KmsResourcePolicyLookup::new(store);
        let resource_policy = lookup.lookup(&arn).unwrap();

        let identity = awsim_iam_policy::parse(
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"kms:Encrypt","Resource":"*"}]}"#,
        )
        .unwrap();

        let context = HashMap::new();
        let req = AuthzRequest {
            principal_arn: "arn:aws:iam::000000000000:user/alice",
            principal_account: "000000000000",
            action: "kms:Encrypt",
            resource_arn: &arn,
            context: &context,
        };
        let scps: Vec<_> = Vec::new();
        let eval_ctx = EvalContext {
            identity_policies: &[identity],
            permissions_boundary: None,
            resource_policy: Some(&resource_policy),
            scps: &scps,
            session_policy: None,
        };
        assert!(matches!(evaluate(&req, &eval_ctx), Decision::Allow));
    }
}
