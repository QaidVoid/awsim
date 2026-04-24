use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;

use crate::state::S3State;

pub struct S3ResourcePolicyLookup {
    store: AccountRegionStore<S3State>,
}

impl S3ResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<S3State>) -> Self {
        Self { store }
    }
}

fn extract_bucket(arn: &str) -> Option<&str> {
    let rest = arn.strip_prefix("arn:aws:s3:::")?;
    let bucket = rest.split('/').next()?;
    if bucket.is_empty() {
        None
    } else {
        Some(bucket)
    }
}

impl ResourcePolicyLookup for S3ResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        let bucket_name = extract_bucket(resource_arn)?;
        for (_, state) in self.store.iter_all() {
            if let Some(bucket) = state.buckets.get(bucket_name) {
                if let Some(policy) = bucket.policy.as_deref() {
                    return awsim_iam_policy::parse(policy).ok();
                }
            }
        }
        None
    }
}
