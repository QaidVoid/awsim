//! SNS topic resource-policy lookup for the gateway AuthzEngine.
//!
//! SNS topics carry an attribute named `Policy` (set via
//! `SetTopicAttributes` with `AttributeName=Policy`) that defines a
//! resource-based policy: who can `sns:Publish`, `sns:Subscribe`,
//! `sns:Receive`, etc. on the topic. The default policy AWS provisions
//! lets only the topic owner publish/subscribe; cross-account access
//! relies entirely on this document.
//!
//! Wiring this lookup into [`awsim_core::AuthzEngine`] for the `sns`
//! service makes the policy evaluator consult the topic's policy
//! alongside the principal's identity policies, so cross-account
//! workflows behave like AWS instead of silently allowing or denying
//! based on identity alone.

use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::SnsState;

pub struct SnsResourcePolicyLookup {
    store: AccountRegionStore<SnsState>,
}

impl SnsResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<SnsState>) -> Self {
        Self { store }
    }
}

impl ResourcePolicyLookup for SnsResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        for (_, state) in self.store.iter_all() {
            if let Some(topic) = state.topics.get(resource_arn)
                && let Some(raw) = topic.attributes.get("Policy")
                && !raw.is_empty()
            {
                return awsim_iam_policy::parse(raw).ok();
            }
        }
        None
    }
}
