use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use awsim_iam_policy::{
    AuthzRequest, ContextValue, Decision, EvalContext, PolicyDocument, evaluate,
};

use crate::error::AwsError;
use crate::router::RequestContext;

#[derive(Clone)]
pub struct ResolvedPrincipal {
    pub arn: String,
    pub account: String,
    pub identity_policies: Vec<PolicyDocument>,
    pub permissions_boundary: Option<PolicyDocument>,
    pub is_root: bool,
    /// Principal tags, surfaced into `aws:PrincipalTag/<key>` for IAM
    /// condition evaluation. Empty for root and for federated principals
    /// without a backing IAM record.
    pub tags: HashMap<String, String>,
    /// Session policy captured at AssumeRole time. Populated by the
    /// STS-aware principal lookup when the resolved credential is an
    /// ASIA token; `None` for long-lived IAM-user keys. Real AWS
    /// narrows assumed-role permissions to the intersection of the
    /// role's identity policies and this document, so the AuthzEngine
    /// surfaces it into `EvalContext::session_policy`.
    pub session_policy: Option<PolicyDocument>,
}

pub trait PrincipalLookup: Send + Sync {
    fn resolve_access_key(&self, access_key: &str) -> Option<ResolvedPrincipal>;

    /// Look up a principal by its ARN. Used by chaining wrappers (the
    /// STS-aware lookup in particular) that hold a role ARN and need
    /// to materialise the role's identity policies. Default
    /// implementation returns `None` so existing impls don't have to
    /// add a stub.
    fn resolve_arn(&self, _arn: &str) -> Option<ResolvedPrincipal> {
        None
    }

    /// Look up the plaintext secret access key for a given access key
    /// ID. Used by the SigV4 verifier on the request path when
    /// `AWSIM_VERIFY_SIGV4` is enabled so the gateway can recompute
    /// the signature and reject impostors. Default returns `None`,
    /// which the verifier treats as "unknown principal" and rejects
    /// with `InvalidClientTokenId`.
    fn resolve_secret(&self, _access_key: &str) -> Option<String> {
        None
    }

    /// Record that a successful resolution just happened. AWS exposes
    /// this through `GetAccessKeyLastUsed`; the gateway hooks it after
    /// every authenticated request so callers see the timestamp slide
    /// forward without an explicit IAM API call. The default is a
    /// no-op so existing implementations (and tests) don't need to
    /// supply one.
    fn record_access_key_used(&self, _access_key: &str, _service: &str, _region: &str) {}
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

/// Resolve a KMS key reference (key id, key ARN, alias, or alias ARN)
/// into the canonical key id. Returns `None` when no such key/alias
/// exists in the given (account, region). Used by service crates that
/// accept a KMS key reference (SNS topics, SQS queues, log groups,
/// etc.) and need to validate the reference against awsim-kms without
/// taking a direct dependency on the kms crate.
pub trait KmsKeyLookup: Send + Sync {
    fn resolve_key(&self, key_ref: &str, account: &str, region: &str) -> Option<String>;
}

/// Resolve a SecretsManager secret reference (name or ARN). Returns
/// `true` when a matching secret exists in the given (account,
/// region). Used by service crates (ECS repositoryCredentials, ECS
/// container secrets[], EventBridge connection auth) that need to
/// validate a secret reference without depending on awsim-secretsmanager.
pub trait SecretLookup: Send + Sync {
    fn secret_exists(&self, secret_ref: &str, account: &str, region: &str) -> bool;
}

/// Resolve an SSM Parameter Store reference (parameter name or ARN).
/// Returns `true` when the parameter exists in the given (account,
/// region). Used by service crates that consume SSM parameter
/// references (ECS container `secrets[].valueFrom`, Lambda layer
/// configuration, etc.) without depending on awsim-ssm.
pub trait ParameterLookup: Send + Sync {
    fn parameter_exists(&self, parameter_ref: &str, account: &str, region: &str) -> bool;
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
    /// Access key that bypasses IAM enforcement and is treated as
    /// root-equivalent. Models the AWS account root credential —
    /// IAM only governs IAM users/roles, not the account itself.
    /// `None` means no bypass key is configured. The admin key is
    /// also not subject to `principal_lookup`, so it works even
    /// before any IAM users exist (bootstrap path).
    pub admin_access_key: Option<String>,
}

impl AuthzEngine {
    pub fn new(enabled: bool) -> Self {
        Self {
            principal_lookup: Arc::new(NoopPrincipalLookup),
            resource_policy_lookups: HashMap::new(),
            grant_lookups: HashMap::new(),
            scp_lookup: None,
            enforced: AtomicBool::new(enabled),
            admin_access_key: None,
        }
    }

    pub fn from_env() -> Self {
        let enabled = std::env::var("AWSIM_IAM_ENFORCE").ok().as_deref() == Some("true");
        let mut engine = Self::new(enabled);
        engine.admin_access_key = std::env::var("AWSIM_ADMIN_ACCESS_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        engine
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

    /// Returns true when `key` matches the configured admin access
    /// key. Constant-time comparison isn't needed: the simulator
    /// doesn't verify signatures, so the key is already trivially
    /// observable to anyone on the loopback interface.
    fn is_admin_key(&self, key: &str) -> bool {
        self.admin_access_key.as_deref() == Some(key)
    }

    /// Public mirror of [`Self::is_admin_key`] for the gateway's
    /// signed-request gate, which needs to let the admin key
    /// through even when no IAM user maps to it (bootstrap path).
    pub fn is_admin_access_key(&self, key: &str) -> bool {
        self.is_admin_key(key)
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

        // Admin key short-circuit. Mirrors how AWS root credentials sit
        // outside IAM: the management UI and bootstrap flows use this
        // key so they keep working once enforcement is on, even before
        // any IAM users exist.
        if self.is_admin_key(access_key) {
            return Ok(());
        }

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

        let context = build_request_context(ctx, &principal);

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
            session_policy: principal.session_policy.as_ref(),
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

impl AuthzEngine {
    /// Authorize the caller to hand `role_arn` to `target_service`.
    ///
    /// Resource-creating operations that bind a role to another
    /// service (Lambda `CreateFunction`, ECS `RunTask`,
    /// CodePipeline `CreatePipeline`, Bedrock model invocation
    /// roles, etc.) must verify the caller holds `iam:PassRole` on
    /// the target role. Mirrors AWS's pre-flight check; returns
    /// `AccessDeniedException` if enforcement is on and the policy
    /// denies. No-op when enforcement is off.
    ///
    /// `target_service` is the AWS service principal that will
    /// assume the role (e.g. `"lambda.amazonaws.com"`,
    /// `"ecs-tasks.amazonaws.com"`). It's threaded into the
    /// condition context as `iam:PassedToService` so policies that
    /// scope `PassRole` by service work correctly.
    pub fn check_pass_role(
        &self,
        ctx: &RequestContext,
        role_arn: &str,
        target_service: &str,
    ) -> Result<(), AwsError> {
        // We do not currently feed `iam:PassedToService` into the
        // condition context (the engine accepts the variable but no
        // call site sets it yet). The standard PassRole check still
        // runs through the normal evaluator path so identity
        // policies and SCPs apply.
        let _ = target_service;
        self.check(ctx, "iam:PassRole", role_arn)
    }
}

/// Build the IAM condition-context map for one request. Populates the
/// AWS-standard variables that the policy evaluator consumes:
///
/// * `aws:CurrentTime` (Date)
/// * `aws:EpochTime` (Number) — same instant as seconds since 1970
/// * `aws:SourceIp` (Ip), when the request carried a recoverable client IP
/// * `aws:SecureTransport` (Bool)
/// * `aws:PrincipalArn` / `aws:PrincipalAccount` — already known to the
///   evaluator's variable resolver but mirrored here so condition lookups
///   that reference them as keys (rare but legal) also see them.
/// * `aws:PrincipalTag/<key>` (String) for every tag on the resolved
///   principal.
fn build_request_context(
    ctx: &RequestContext,
    principal: &ResolvedPrincipal,
) -> HashMap<String, ContextValue> {
    let mut context = HashMap::new();
    let now = chrono::Utc::now();
    context.insert("aws:CurrentTime".to_string(), ContextValue::Date(now));
    context.insert(
        "aws:EpochTime".to_string(),
        ContextValue::Number(now.timestamp() as f64),
    );
    context.insert(
        "aws:SecureTransport".to_string(),
        ContextValue::Bool(ctx.is_secure),
    );
    if let Some(ref ip) = ctx.source_ip {
        context.insert("aws:SourceIp".to_string(), ContextValue::Ip(ip.clone()));
    }
    context.insert(
        "aws:PrincipalArn".to_string(),
        ContextValue::String(principal.arn.clone()),
    );
    context.insert(
        "aws:PrincipalAccount".to_string(),
        ContextValue::String(principal.account.clone()),
    );
    for (k, v) in &principal.tags {
        context.insert(
            format!("aws:PrincipalTag/{k}"),
            ContextValue::String(v.clone()),
        );
    }
    context
}
