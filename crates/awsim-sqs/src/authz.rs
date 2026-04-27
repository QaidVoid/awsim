use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::SqsState;

pub struct SqsResourcePolicyLookup {
    store: AccountRegionStore<SqsState>,
}

impl SqsResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<SqsState>) -> Self {
        Self { store }
    }
}

impl ResourcePolicyLookup for SqsResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        for (_, state) in self.store.iter_all() {
            if let Some(name) = state.queue_name_by_arn(resource_arn)
                && let Some(queue) = state.queues.get(&name)
                && let Some(raw) = queue.attributes.get("Policy")
            {
                return awsim_iam_policy::parse(raw).ok();
            }
        }
        None
    }
}
