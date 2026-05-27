//! Cross-service authz bridge: surfaces ECR registry- and repository-
//! level policies to the gateway's [`AuthzEngine`] so cross-account
//! `ecr:GetDownloadUrlForLayer` / `ecr:BatchGetImage` calls that
//! require an explicit grant succeed when one is attached.

use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::EcrState;

pub struct EcrResourcePolicyLookup {
    store: AccountRegionStore<EcrState>,
}

impl EcrResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<EcrState>) -> Self {
        Self { store }
    }
}

/// Extract `(account, region, repository)` from an ECR ARN of the
/// form `arn:aws:ecr:{region}:{account}:repository/{name}`. Returns
/// `None` for non-repository ARNs (e.g. registry ARNs without a
/// `:repository/` segment).
fn parse_repository_arn(arn: &str) -> Option<(String, String, String)> {
    let rest = arn.strip_prefix("arn:aws:ecr:")?;
    let mut parts = rest.splitn(3, ':');
    let region = parts.next()?;
    let account = parts.next()?;
    let resource = parts.next()?;
    let name = resource.strip_prefix("repository/")?;
    if region.is_empty() || account.is_empty() || name.is_empty() {
        return None;
    }
    Some((account.to_string(), region.to_string(), name.to_string()))
}

/// Extract `(account, region)` from an ECR registry ARN of the form
/// `arn:aws:ecr:{region}:{account}:registry/{account}`.
fn parse_registry_arn(arn: &str) -> Option<(String, String)> {
    let rest = arn.strip_prefix("arn:aws:ecr:")?;
    let mut parts = rest.splitn(3, ':');
    let region = parts.next()?;
    let account = parts.next()?;
    let resource = parts.next()?;
    if !resource.starts_with("registry") {
        return None;
    }
    Some((account.to_string(), region.to_string()))
}

impl ResourcePolicyLookup for EcrResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        if let Some((account, region, repo)) = parse_repository_arn(resource_arn) {
            let state = self.store.get(&account, &region);
            let raw = state
                .repositories
                .get(&repo)
                .and_then(|r| r.value().repository_policy.clone())?;
            return awsim_iam_policy::parse(&raw).ok();
        }
        if let Some((account, region)) = parse_registry_arn(resource_arn) {
            let state = self.store.get(&account, &region);
            let raw = state.registry_policy.get("default").map(|e| e.clone())?;
            return awsim_iam_policy::parse(&raw).ok();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Repository;

    fn allow_all_policy() -> String {
        r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"ecr:*","Resource":"*"}]}"#
            .to_string()
    }

    #[test]
    fn returns_repository_policy_when_set() {
        let store: AccountRegionStore<EcrState> = AccountRegionStore::new();
        let state = store.get("000000000000", "us-east-1");
        state.repositories.insert(
            "demo".into(),
            Repository {
                name: "demo".into(),
                arn: "arn:aws:ecr:us-east-1:000000000000:repository/demo".into(),
                registry_id: "000000000000".into(),
                repository_uri: "000000000000.dkr.ecr.us-east-1.amazonaws.com/demo".into(),
                images: Vec::new(),
                layers: dashmap::DashMap::new(),
                created_at: "1970-01-01T00:00:00Z".into(),
                image_tag_mutability: "MUTABLE".into(),
                tags: std::collections::HashMap::new(),
                lifecycle_policy: None,
                lifecycle_policy_preview: None,
                repository_policy: Some(allow_all_policy()),
                scan_on_push: false,
                encryption_type: "AES256".into(),
                kms_key: None,
            },
        );

        let lookup = EcrResourcePolicyLookup::new(store);
        let policy = lookup
            .lookup("arn:aws:ecr:us-east-1:000000000000:repository/demo")
            .expect("policy resolves");
        assert!(!policy.statements.is_empty());
    }

    #[test]
    fn returns_none_for_unknown_repository() {
        let store: AccountRegionStore<EcrState> = AccountRegionStore::new();
        let lookup = EcrResourcePolicyLookup::new(store);
        assert!(
            lookup
                .lookup("arn:aws:ecr:us-east-1:000000000000:repository/missing")
                .is_none()
        );
    }

    #[test]
    fn returns_registry_policy_when_set() {
        let store: AccountRegionStore<EcrState> = AccountRegionStore::new();
        let state = store.get("000000000000", "us-east-1");
        state
            .registry_policy
            .insert("default".to_string(), allow_all_policy());

        let lookup = EcrResourcePolicyLookup::new(store);
        let policy = lookup
            .lookup("arn:aws:ecr:us-east-1:000000000000:registry/000000000000")
            .expect("registry policy resolves");
        assert!(!policy.statements.is_empty());
    }

    #[test]
    fn parses_repository_arn_components() {
        let parts =
            parse_repository_arn("arn:aws:ecr:us-east-1:000000000000:repository/demo").unwrap();
        assert_eq!(parts.0, "000000000000");
        assert_eq!(parts.1, "us-east-1");
        assert_eq!(parts.2, "demo");
    }

    #[test]
    fn rejects_non_ecr_arn() {
        assert!(parse_repository_arn("arn:aws:sqs:us-east-1:0:q").is_none());
    }
}
