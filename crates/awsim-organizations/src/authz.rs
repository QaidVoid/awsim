use std::sync::Arc;

use awsim_core::{AccountRegionStore, ScpLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::OrganizationsState;

pub struct OrganizationsScpLookup {
    store: AccountRegionStore<OrganizationsState>,
    default_account: String,
}

impl OrganizationsScpLookup {
    pub fn new(
        store: AccountRegionStore<OrganizationsState>,
        default_account: impl Into<String>,
    ) -> Self {
        Self {
            store,
            default_account: default_account.into(),
        }
    }
}

fn parse_account_from_arn(arn: &str) -> Option<String> {
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    if parts.len() < 5 {
        return None;
    }
    let account = parts[4];
    if account.is_empty() {
        None
    } else {
        Some(account.to_string())
    }
}

impl ScpLookup for OrganizationsScpLookup {
    fn lookup(&self, principal_arn: &str) -> Vec<PolicyDocument> {
        let account_id =
            parse_account_from_arn(principal_arn).unwrap_or_else(|| self.default_account.clone());
        let state: Arc<OrganizationsState> = self.store.get(&account_id, "global");

        let mut policy_contents: Vec<String> = Vec::new();
        let mut node_id = account_id.clone();
        let mut seen = std::collections::HashSet::new();

        loop {
            if !seen.insert(node_id.clone()) {
                break;
            }
            if let Some(attached) = state.policy_attachments.get(&node_id) {
                for policy_id in attached.iter() {
                    if let Some(p) = state.policies.get(policy_id)
                        && p.policy_type == "SERVICE_CONTROL_POLICY" {
                            policy_contents.push(p.content.clone());
                        }
                }
            }
            match state.parents.get(&node_id) {
                Some(p) => node_id = p.clone(),
                None => break,
            }
        }

        policy_contents
            .into_iter()
            .filter_map(|doc| awsim_iam_policy::parse(&doc).ok())
            .collect()
    }
}
