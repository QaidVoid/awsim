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

    pub fn resolve_arn(&self, arn: &str) -> Option<ResolvedPrincipal> {
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
                });
            }
        }
        None
    }
}

impl PrincipalLookup for IamPrincipalLookup {
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
                    is_root: false,
                    tags: user.tags.clone(),
                });
            }
        }

        debug!(access_key = %access_key, "No IAM user found for access key");
        None
    }
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
