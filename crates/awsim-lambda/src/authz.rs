use awsim_core::{AccountRegionStore, ResourcePolicyLookup};
use awsim_iam_policy::PolicyDocument;
use serde_json::json;

use crate::state::LambdaState;

pub struct LambdaResourcePolicyLookup {
    store: AccountRegionStore<LambdaState>,
}

impl LambdaResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<LambdaState>) -> Self {
        Self { store }
    }
}

fn extract_function_name(arn: &str) -> Option<String> {
    let rest = arn.strip_prefix("arn:aws:lambda:")?;
    let parts: Vec<&str> = rest.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let resource = parts[2];
    let after = resource.strip_prefix("function:")?;
    let name = after.split(':').next()?;
    Some(name.to_string())
}

impl ResourcePolicyLookup for LambdaResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        let function_name = extract_function_name(resource_arn)?;
        for (_, state) in self.store.iter_all() {
            if let Some(func) = state.functions.get(&function_name) {
                if func.policy_statements.is_empty() {
                    return None;
                }
                let statements: Vec<serde_json::Value> =
                    func.policy_statements.values().cloned().collect();
                let doc = json!({
                    "Version": "2012-10-17",
                    "Statement": statements,
                });
                return awsim_iam_policy::parse(&doc.to_string()).ok();
            }
        }
        None
    }
}
