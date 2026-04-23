use std::collections::HashMap;
use std::sync::Arc;

use awsim_iam_policy::{
    AuthzRequest, ContextValue, Decision, EvalContext, PolicyDocument, evaluate,
};

use crate::error::AwsError;
use crate::router::RequestContext;

pub struct ResolvedPrincipal {
    pub arn: String,
    pub account: String,
    pub identity_policies: Vec<PolicyDocument>,
    pub permissions_boundary: Option<PolicyDocument>,
    pub is_root: bool,
}

pub trait PrincipalLookup: Send + Sync {
    fn resolve_access_key(&self, access_key: &str) -> Option<ResolvedPrincipal>;
}

pub trait ResourcePolicyLookup: Send + Sync {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument>;
}

pub trait ScpLookup: Send + Sync {
    fn lookup(&self, principal_arn: &str) -> Vec<PolicyDocument>;
}

pub struct NoopPrincipalLookup;

impl PrincipalLookup for NoopPrincipalLookup {
    fn resolve_access_key(&self, _access_key: &str) -> Option<ResolvedPrincipal> {
        None
    }
}

pub struct AuthzEngine {
    pub principal_lookup: Arc<dyn PrincipalLookup>,
    pub resource_policy_lookups: HashMap<String, Arc<dyn ResourcePolicyLookup>>,
    pub scp_lookup: Option<Arc<dyn ScpLookup>>,
    pub enabled: bool,
}

impl AuthzEngine {
    pub fn new(enabled: bool) -> Self {
        Self {
            principal_lookup: Arc::new(NoopPrincipalLookup),
            resource_policy_lookups: HashMap::new(),
            scp_lookup: None,
            enabled,
        }
    }

    pub fn from_env() -> Self {
        let enabled = std::env::var("AWSIM_IAM_ENFORCE")
            .ok()
            .as_deref()
            == Some("true");
        Self::new(enabled)
    }

    pub fn check(
        &self,
        ctx: &RequestContext,
        action: &str,
        resource: &str,
    ) -> Result<(), AwsError> {
        if !self.enabled {
            return Ok(());
        }

        let access_key = match ctx.access_key.as_deref() {
            Some(k) if !k.is_empty() => k,
            _ => {
                return Err(AwsError::access_denied_for(
                    action,
                    "anonymous",
                    resource,
                ));
            }
        };

        let principal = match self.principal_lookup.resolve_access_key(access_key) {
            Some(p) => p,
            None => {
                return Err(AwsError::access_denied_for(
                    action,
                    &format!("AccessKey:{access_key}"),
                    resource,
                ));
            }
        };

        if principal.is_root {
            return Ok(());
        }

        let resource_policy = self
            .resource_policy_lookups
            .get(&ctx.service)
            .and_then(|lookup| lookup.lookup(resource));

        let context: HashMap<String, ContextValue> = HashMap::new();

        let req = AuthzRequest {
            principal_arn: &principal.arn,
            principal_account: &principal.account,
            action,
            resource_arn: resource,
            context: &context,
        };

        let scps: Vec<PolicyDocument> = self
            .scp_lookup
            .as_ref()
            .map(|l| l.lookup(&principal.arn))
            .unwrap_or_default();

        let eval_ctx = EvalContext {
            identity_policies: &principal.identity_policies,
            permissions_boundary: principal.permissions_boundary.as_ref(),
            resource_policy: resource_policy.as_ref(),
            scps: &scps,
            session_policy: None,
        };

        match evaluate(&req, &eval_ctx) {
            Decision::Allow => Ok(()),
            Decision::ExplicitDeny | Decision::ImplicitDeny => Err(
                AwsError::access_denied_for(action, &principal.arn, resource),
            ),
        }
    }
}

impl Default for AuthzEngine {
    fn default() -> Self {
        Self::new(false)
    }
}
