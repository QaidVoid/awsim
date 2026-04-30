use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Service-specific authorization side-channel. Used by KMS grants today —
/// a grant is an out-of-band Allow that lets a principal perform listed
/// operations on a resource even if the identity policy and key policy
/// would otherwise deny. Returns `true` when at least one grant matches the
/// principal + action + resource.
pub trait GrantLookup: Send + Sync {
    fn allows(&self, principal_arn: &str, action: &str, resource_arn: &str) -> bool;
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
    pub grant_lookups: HashMap<String, Arc<dyn GrantLookup>>,
    pub scp_lookup: Option<Arc<dyn ScpLookup>>,
    /// Atomic so the runtime config can flip enforcement on/off
    /// without rebuilding the engine. Reads on the request path are
    /// `Relaxed` since enforcement-toggle ordering vs in-flight
    /// requests doesn't have correctness implications.
    enforced: AtomicBool,
}

impl AuthzEngine {
    pub fn new(enabled: bool) -> Self {
        Self {
            principal_lookup: Arc::new(NoopPrincipalLookup),
            resource_policy_lookups: HashMap::new(),
            grant_lookups: HashMap::new(),
            scp_lookup: None,
            enforced: AtomicBool::new(enabled),
        }
    }

    pub fn from_env() -> Self {
        let enabled = std::env::var("AWSIM_IAM_ENFORCE").ok().as_deref() == Some("true");
        Self::new(enabled)
    }

    /// Enable or disable IAM enforcement. Hot-reload-safe: in-flight
    /// requests already past the `enabled` check see the previous
    /// value, which is fine — we don't make any policy decisions
    /// that depend on this being a stable view across an entire
    /// request.
    pub fn set_enabled(&self, enabled: bool) {
        self.enforced.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enforced.load(Ordering::Relaxed)
    }

    pub fn check(
        &self,
        ctx: &RequestContext,
        action: &str,
        resource: &str,
    ) -> Result<(), AwsError> {
        if !self.enabled() {
            return Ok(());
        }

        let access_key = match ctx.access_key.as_deref() {
            Some(k) if !k.is_empty() => k,
            _ => {
                return Err(AwsError::access_denied_for(action, "anonymous", resource));
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
            // Implicit deny is the natural outcome when neither the identity
            // policy nor the resource policy explicitly allows. KMS grants
            // are an out-of-band Allow path — give them a chance before we
            // actually fail the request. Explicit deny still wins absolutely.
            Decision::ImplicitDeny => {
                if let Some(lookup) = self.grant_lookups.get(&ctx.service)
                    && lookup.allows(&principal.arn, action, resource)
                {
                    return Ok(());
                }
                Err(AwsError::access_denied_for(
                    action,
                    &principal.arn,
                    resource,
                ))
            }
            Decision::ExplicitDeny => Err(AwsError::access_denied_for(
                action,
                &principal.arn,
                resource,
            )),
        }
    }
}

impl Default for AuthzEngine {
    fn default() -> Self {
        Self::new(false)
    }
}
