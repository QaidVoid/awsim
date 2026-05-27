use std::sync::Arc;

use awsim_core::{AccountRegionStore, PrincipalLookup, ResolvedPrincipal};
use awsim_iam_policy::PolicyDocument;
use tracing::debug;

use crate::state::IamState;

pub struct IamPrincipalLookup {
    store: AccountRegionStore<IamState>,
}

impl IamPrincipalLookup {
    pub fn new(store: AccountRegionStore<IamState>) -> Self {
        Self { store }
    }

    fn resolve_arn_impl(&self, arn: &str) -> Option<ResolvedPrincipal> {
        for ((account_id, _region), state) in self.store.iter_all() {
            if let Some(user) = state
                .users
                .iter()
                .find(|entry| entry.value().arn == arn)
                .map(|entry| entry.value().clone())
            {
                let identity_policies = collect_identity_policies(&state, &user);
                let permissions_boundary = state
                    .user_permissions_boundaries
                    .get(&user.user_name)
                    .and_then(|entry| {
                        let boundary_arn = entry.value().clone();
                        state
                            .policies
                            .get(&boundary_arn)
                            .and_then(|p| parse_policy_document(&p.value().policy_document))
                    });
                return Some(ResolvedPrincipal {
                    arn: user.arn.clone(),
                    account: account_id.clone(),
                    identity_policies,
                    permissions_boundary,
                    is_root: false,
                    tags: user.tags.clone(),
                    session_policy: None,
                });
            }
            if let Some(role) = state
                .roles
                .iter()
                .find(|entry| entry.value().arn == arn)
                .map(|entry| entry.value().clone())
            {
                let identity_policies = collect_role_identity_policies(&state, &role);
                let permissions_boundary = state
                    .role_permissions_boundaries
                    .get(&role.role_name)
                    .and_then(|entry| {
                        let boundary_arn = entry.value().clone();
                        state
                            .policies
                            .get(&boundary_arn)
                            .and_then(|p| parse_policy_document(&p.value().policy_document))
                    });
                return Some(ResolvedPrincipal {
                    arn: role.arn.clone(),
                    account: account_id.clone(),
                    identity_policies,
                    permissions_boundary,
                    is_root: false,
                    tags: role.tags.clone(),
                    session_policy: None,
                });
            }
        }
        None
    }
}

impl PrincipalLookup for IamPrincipalLookup {
    fn resolve_arn(&self, arn: &str) -> Option<ResolvedPrincipal> {
        self.resolve_arn_impl(arn)
    }

    fn resolve_access_key(&self, access_key: &str) -> Option<ResolvedPrincipal> {
        for ((account_id, _region), state) in self.store.iter_all() {
            if let Some(user) = find_user_with_key(&state, access_key) {
                let identity_policies = collect_identity_policies(&state, &user);
                let permissions_boundary = state
                    .user_permissions_boundaries
                    .get(&user.user_name)
                    .and_then(|entry| {
                        let boundary_arn = entry.value().clone();
                        state
                            .policies
                            .get(&boundary_arn)
                            .and_then(|p| parse_policy_document(&p.value().policy_document))
                    });

                return Some(ResolvedPrincipal {
                    arn: user.arn.clone(),
                    account: account_id.clone(),
                    identity_policies,
                    permissions_boundary,
                    // The user named "root" is the account owner that
                    // first-run setup provisions. Real AWS treats root
                    // as outside the IAM principal hierarchy: its
                    // credentials always allow everything, regardless
                    // of attached policies. AWSim mirrors that by
                    // flagging the principal so the AuthzEngine
                    // short-circuits at the root-bypass check.
                    is_root: user.user_name == crate::ROOT_USERNAME,
                    tags: user.tags.clone(),
                    session_policy: None,
                });
            }
        }

        debug!(access_key = %access_key, "No IAM user found for access key");
        None
    }

    fn resolve_secret(&self, access_key: &str) -> Option<String> {
        for (_, state) in self.store.iter_all() {
            for entry in state.users.iter() {
                if let Some(k) = entry
                    .value()
                    .access_keys
                    .iter()
                    .find(|k| k.access_key_id == access_key)
                {
                    return Some(k.secret_access_key.clone());
                }
            }
        }
        None
    }

    fn record_access_key_used(&self, access_key: &str, service: &str, region: &str) {
        use crate::state::AccessKeyLastUsed;
        let now = chrono_now_iso8601();
        for (_, state) in self.store.iter_all() {
            // Cheap pre-check: skip states that don't own the key.
            let owns = state.users.iter().any(|e| {
                e.value()
                    .access_keys
                    .iter()
                    .any(|k| k.access_key_id == access_key)
            });
            if !owns {
                continue;
            }
            state.access_key_last_used.insert(
                access_key.to_string(),
                AccessKeyLastUsed {
                    last_used_date: Some(now.clone()),
                    service_name: service.to_string(),
                    region: region.to_string(),
                },
            );
            return;
        }
    }
}

fn chrono_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Cheap ISO-8601 second-resolution formatter. Avoids pulling in
    // chrono just for this hook.
    let mut s = secs as i64;
    let sec = s % 60;
    s /= 60;
    let min = s % 60;
    s /= 60;
    let hour = s % 24;
    let mut days = s / 24;
    let (mut year, mut month, mut day) = (1970, 1, 1);
    while days > 0 {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let dpy = if leap { 366 } else { 365 };
        if days >= dpy {
            days -= dpy;
            year += 1;
            continue;
        }
        let months = if leap {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };
        for m in &months {
            if days >= *m {
                days -= *m;
                month += 1;
            } else {
                break;
            }
        }
        day = (days + 1) as i32;
        break;
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn find_user_with_key(state: &IamState, access_key: &str) -> Option<crate::state::User> {
    state.users.iter().find_map(|entry| {
        let user = entry.value();
        if user
            .access_keys
            .iter()
            .any(|k| k.access_key_id == access_key)
        {
            Some(user.clone())
        } else {
            None
        }
    })
}

fn collect_identity_policies(state: &IamState, user: &crate::state::User) -> Vec<PolicyDocument> {
    let mut docs = Vec::new();

    for raw in user.inline_policies.values() {
        if let Some(doc) = parse_policy_document(raw) {
            docs.push(doc);
        }
    }

    for arn in &user.attached_policies {
        if let Some(policy) = state.policies.get(arn)
            && let Some(doc) = parse_policy_document(&policy.value().policy_document)
        {
            docs.push(doc);
        }
    }

    for group_name in &user.groups {
        if let Some(group) = state.groups.get(group_name) {
            let group = group.value();
            for raw in group.inline_policies.values() {
                if let Some(doc) = parse_policy_document(raw) {
                    docs.push(doc);
                }
            }
            for arn in &group.attached_policies {
                if let Some(policy) = state.policies.get(arn)
                    && let Some(doc) = parse_policy_document(&policy.value().policy_document)
                {
                    docs.push(doc);
                }
            }
        }
    }

    docs
}

fn collect_role_identity_policies(
    state: &IamState,
    role: &crate::state::Role,
) -> Vec<PolicyDocument> {
    let mut docs = Vec::new();
    for raw in role.inline_policies.values() {
        if let Some(doc) = parse_policy_document(raw) {
            docs.push(doc);
        }
    }
    for arn in &role.attached_policies {
        if let Some(policy) = state.policies.get(arn)
            && let Some(doc) = parse_policy_document(&policy.value().policy_document)
        {
            docs.push(doc);
        }
    }
    docs
}

fn parse_policy_document(raw: &str) -> Option<PolicyDocument> {
    match awsim_iam_policy::parse(raw) {
        Ok(doc) => Some(doc),
        Err(e) => {
            debug!(error = %e, "Failed to parse policy document");
            None
        }
    }
}

#[allow(dead_code)]
pub fn _ensure_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IamPrincipalLookup>();
    assert_send_sync::<Arc<IamPrincipalLookup>>();
}

#[cfg(test)]
mod last_used_tests {
    use super::*;
    use crate::state::{AccessKey, IamState, User};
    use awsim_core::AccountRegionStore;
    use awsim_core::authz::PrincipalLookup;
    use std::collections::HashMap;

    fn make_store_with_user(key_id: &str) -> AccountRegionStore<IamState> {
        let store: AccountRegionStore<IamState> = AccountRegionStore::default();
        let state = store.get("000000000000", "us-east-1");
        let user = User {
            user_name: "alice".into(),
            user_id: "AIDATEST00000000000A".into(),
            arn: "arn:aws:iam::000000000000:user/alice".into(),
            path: "/".into(),
            create_date: "1970-01-01T00:00:00Z".into(),
            access_keys: vec![AccessKey {
                access_key_id: key_id.into(),
                secret_access_key: "secret".into(),
                status: "Active".into(),
                create_date: "1970-01-01T00:00:00Z".into(),
            }],
            attached_policies: vec![],
            inline_policies: HashMap::new(),
            groups: vec![],
            tags: HashMap::new(),
            mfa_devices: vec![],
            ssh_public_keys: vec![],
            password_last_used: None,
        };
        state.users.insert("alice".to_string(), user);
        store
    }

    #[test]
    fn record_access_key_used_sets_service_region_and_date() {
        let key = "AKIATEST";
        let store = make_store_with_user(key);
        let lookup = IamPrincipalLookup::new(store.clone());
        lookup.record_access_key_used(key, "kms", "us-east-1");
        let state = store.get("000000000000", "us-east-1");
        let row = state.access_key_last_used.get(key).unwrap();
        assert_eq!(row.value().service_name, "kms");
        assert_eq!(row.value().region, "us-east-1");
        let date = row.value().last_used_date.clone().unwrap();
        assert!(date.ends_with('Z'));
        assert!(date.len() >= 19);
    }

    #[test]
    fn record_access_key_used_ignores_unknown_keys() {
        let store: AccountRegionStore<IamState> = AccountRegionStore::default();
        let lookup = IamPrincipalLookup::new(store.clone());
        lookup.record_access_key_used("AKIAUNKNOWN", "s3", "us-east-1");
        let state = store.get("000000000000", "us-east-1");
        assert!(state.access_key_last_used.is_empty());
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::state::{AccessKey, IamState, Policy, User};
    use awsim_core::AccountRegionStore;
    use awsim_core::authz::AuthzEngine;
    use awsim_core::router::RequestContext;
    use std::collections::HashMap;
    use std::sync::Arc;

    const KEY: &str = "AKIATESTBOUND0001";
    const BOUNDARY_ARN: &str = "arn:aws:iam::000000000000:policy/Boundary";

    fn policy_doc(allow_action: &str) -> String {
        format!(
            r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Action":"{allow_action}","Resource":"*"}}]}}"#
        )
    }

    fn make_store(
        identity_action: &str,
        boundary_action: Option<&str>,
    ) -> AccountRegionStore<IamState> {
        let store: AccountRegionStore<IamState> = AccountRegionStore::default();
        let state = store.get("000000000000", "us-east-1");

        let mut inline = HashMap::new();
        inline.insert("inline".to_string(), policy_doc(identity_action));

        let user = User {
            user_name: "alice".into(),
            user_id: "AIDATESTBOUND00000A".into(),
            arn: "arn:aws:iam::000000000000:user/alice".into(),
            path: "/".into(),
            create_date: "1970-01-01T00:00:00Z".into(),
            access_keys: vec![AccessKey {
                access_key_id: KEY.into(),
                secret_access_key: "secret".into(),
                status: "Active".into(),
                create_date: "1970-01-01T00:00:00Z".into(),
            }],
            attached_policies: vec![],
            inline_policies: inline,
            groups: vec![],
            tags: HashMap::new(),
            mfa_devices: vec![],
            ssh_public_keys: vec![],
            password_last_used: None,
        };
        state.users.insert("alice".to_string(), user);

        if let Some(action) = boundary_action {
            state.policies.insert(
                BOUNDARY_ARN.to_string(),
                Policy {
                    policy_name: "Boundary".into(),
                    policy_id: "ANPATESTBOUND0000A".into(),
                    arn: BOUNDARY_ARN.into(),
                    path: "/".into(),
                    description: None,
                    policy_document: policy_doc(action),
                    create_date: "1970-01-01T00:00:00Z".into(),
                    update_date: "1970-01-01T00:00:00Z".into(),
                    attachment_count: 1,
                    versions: vec![],
                    default_version_id: "v1".into(),
                    tags: HashMap::new(),
                },
            );
            state
                .user_permissions_boundaries
                .insert("alice".to_string(), BOUNDARY_ARN.to_string());
        }

        store
    }

    fn engine_for(store: AccountRegionStore<IamState>) -> AuthzEngine {
        let mut engine = AuthzEngine::new(true);
        engine.principal_lookup = Arc::new(IamPrincipalLookup::new(store));
        engine
    }

    fn ctx() -> RequestContext {
        let mut c = RequestContext::new("kms", "us-east-1");
        c.access_key = Some(KEY.into());
        c
    }

    #[test]
    fn boundary_denies_when_action_outside_cap() {
        let store = make_store("kms:Decrypt", Some("s3:GetObject"));
        let engine = engine_for(store);
        let err = engine
            .check(
                &ctx(),
                "kms:Decrypt",
                "arn:aws:kms:us-east-1:000000000000:key/abc",
            )
            .expect_err("boundary should deny outside its allow set");
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn boundary_allows_when_action_inside_cap() {
        let store = make_store("kms:Decrypt", Some("kms:Decrypt"));
        let engine = engine_for(store);
        engine
            .check(
                &ctx(),
                "kms:Decrypt",
                "arn:aws:kms:us-east-1:000000000000:key/abc",
            )
            .expect("boundary covers identity allow");
    }

    #[test]
    fn no_boundary_falls_back_to_identity_policy() {
        let store = make_store("kms:Decrypt", None);
        let engine = engine_for(store);
        engine
            .check(
                &ctx(),
                "kms:Decrypt",
                "arn:aws:kms:us-east-1:000000000000:key/abc",
            )
            .expect("no boundary, identity allow is sufficient");
    }
}
