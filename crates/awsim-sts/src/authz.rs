//! `PrincipalLookup` wrapper that knows about STS-issued temp creds.
//!
//! The IAM principal lookup only walks IAM users' long-lived access
//! keys, so an `ASIA...` key issued by `AssumeRole` resolves to no
//! principal and the request is denied before policies are
//! evaluated. This wrapper checks the [`StsSessionStore`] first; if a
//! session is present it materialises the assumed-role principal —
//! ARN, account, and the role's identity policies — by delegating
//! the role-by-ARN resolution to the inner lookup. Misses fall
//! through unchanged.

use std::sync::Arc;

use awsim_core::{PrincipalLookup, ResolvedPrincipal};

use crate::sessions::StsSessionStore;

pub struct StsAwarePrincipalLookup {
    sessions: Arc<StsSessionStore>,
    inner: Arc<dyn PrincipalLookup>,
}

impl StsAwarePrincipalLookup {
    pub fn new(sessions: Arc<StsSessionStore>, inner: Arc<dyn PrincipalLookup>) -> Self {
        Self { sessions, inner }
    }
}

impl PrincipalLookup for StsAwarePrincipalLookup {
    fn resolve_access_key(&self, access_key: &str) -> Option<ResolvedPrincipal> {
        if let Some(session) = self.sessions.lookup(access_key) {
            // Resolve the underlying role to inherit its identity policies
            // and permissions boundary. If the role has been deleted since
            // the session was issued, the authz engine can't make a
            // policy decision — fall through so the request is denied
            // for "no such principal" rather than an Allow with empty
            // policies (which would silently behave like `Resource: *`
            // for anything explicitly matching elsewhere).
            let role = self.inner.resolve_arn(&session.role_arn)?;
            return Some(ResolvedPrincipal {
                arn: format!(
                    "arn:aws:sts::{}:assumed-role/{}/{}",
                    session.account_id, session.role_name, session.session_name
                ),
                account: session.account_id.clone(),
                identity_policies: role.identity_policies,
                permissions_boundary: role.permissions_boundary,
                is_root: false,
                tags: role.tags,
            });
        }
        self.inner.resolve_access_key(access_key)
    }

    fn resolve_arn(&self, arn: &str) -> Option<ResolvedPrincipal> {
        self.inner.resolve_arn(arn)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use awsim_core::{PrincipalLookup, ResolvedPrincipal};

    use super::StsAwarePrincipalLookup;
    use crate::sessions::{AssumedRoleSession, StsSessionStore};

    struct FakeIam {
        role_by_arn: HashMap<String, ResolvedPrincipal>,
        user_by_key: HashMap<String, ResolvedPrincipal>,
    }

    impl PrincipalLookup for FakeIam {
        fn resolve_access_key(&self, key: &str) -> Option<ResolvedPrincipal> {
            self.user_by_key.get(key).cloned()
        }
        fn resolve_arn(&self, arn: &str) -> Option<ResolvedPrincipal> {
            self.role_by_arn.get(arn).cloned()
        }
    }

    fn principal(arn: &str, account: &str) -> ResolvedPrincipal {
        ResolvedPrincipal {
            arn: arn.to_string(),
            account: account.to_string(),
            identity_policies: vec![],
            permissions_boundary: None,
            is_root: false,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn session_hit_returns_assumed_role_arn() {
        let role_arn = "arn:aws:iam::000000000000:role/AppAuthRole";
        let mut role_map = HashMap::new();
        role_map.insert(role_arn.to_string(), principal(role_arn, "000000000000"));
        let iam = Arc::new(FakeIam {
            role_by_arn: role_map,
            user_by_key: HashMap::new(),
        }) as Arc<dyn PrincipalLookup>;

        let sessions = Arc::new(StsSessionStore::new());
        sessions.record(AssumedRoleSession {
            access_key: "ASIATEST".to_string(),
            role_arn: role_arn.to_string(),
            role_name: "AppAuthRole".to_string(),
            session_name: "session1".to_string(),
            account_id: "000000000000".to_string(),
            assumed_role_id: "AROAFAKE:session1".to_string(),
            expiry: None,
        });

        let lookup = StsAwarePrincipalLookup::new(sessions, iam);
        let p = lookup.resolve_access_key("ASIATEST").expect("hit");
        assert_eq!(
            p.arn,
            "arn:aws:sts::000000000000:assumed-role/AppAuthRole/session1"
        );
        assert_eq!(p.account, "000000000000");
        assert!(!p.is_root);
    }

    #[test]
    fn session_miss_falls_through_to_iam() {
        let mut user_map = HashMap::new();
        user_map.insert("AKIA1".to_string(), principal("arn:aws:iam::1:user/u", "1"));
        let iam = Arc::new(FakeIam {
            role_by_arn: HashMap::new(),
            user_by_key: user_map,
        }) as Arc<dyn PrincipalLookup>;

        let sessions = Arc::new(StsSessionStore::new());
        let lookup = StsAwarePrincipalLookup::new(sessions, iam);
        assert_eq!(
            lookup.resolve_access_key("AKIA1").expect("hit").arn,
            "arn:aws:iam::1:user/u"
        );
    }

    #[test]
    fn session_with_deleted_role_does_not_silently_allow() {
        // The session is recorded but the role no longer resolves —
        // simulating role deletion mid-session. The wrapper should
        // return None so the authz engine fails closed.
        let iam = Arc::new(FakeIam {
            role_by_arn: HashMap::new(),
            user_by_key: HashMap::new(),
        }) as Arc<dyn PrincipalLookup>;

        let sessions = Arc::new(StsSessionStore::new());
        sessions.record(AssumedRoleSession {
            access_key: "ASIA-ORPHAN".to_string(),
            role_arn: "arn:aws:iam::1:role/Gone".to_string(),
            role_name: "Gone".to_string(),
            session_name: "s".to_string(),
            account_id: "1".to_string(),
            assumed_role_id: "A:s".to_string(),
            expiry: None,
        });

        let lookup = StsAwarePrincipalLookup::new(sessions, iam);
        assert!(lookup.resolve_access_key("ASIA-ORPHAN").is_none());
    }
}
