use std::sync::{Arc, OnceLock};

use awsim_core::{AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::{Value, json};
use tracing::debug;

pub mod authz;
pub mod sessions;

pub use authz::StsAwarePrincipalLookup;
pub use sessions::{AssumedRoleSession, StsSessionStore};

/// Look up the trust policy document for a role so AssumeRole can decide
/// whether the calling principal is allowed to assume it.
///
/// The resolver returns a JSON-string `AssumeRolePolicyDocument`. None means
/// "no such role in the given account", which AssumeRole treats the same as
/// "trust policy denies": both surface as `AccessDenied` rather than leaking
/// whether the role exists.
pub trait TrustPolicyResolver: Send + Sync {
    fn resolve(&self, account_id: &str, role_arn: &str) -> Option<String>;
}

pub struct StsService {
    trust_policy: OnceLock<Arc<dyn TrustPolicyResolver>>,
    /// Tracks credentials this service has issued so signed requests
    /// using them resolve to the right principal under IAM enforcement
    /// and so `GetCallerIdentity` reports the assumed-role ARN
    /// instead of falling back to a synthesised IAM-user shape.
    /// Default-constructed empty; share via [`set_session_store`] when
    /// you want STS-issued creds usable across the gateway.
    sessions: Arc<StsSessionStore>,
}

impl StsService {
    pub fn new() -> Self {
        Self::with_session_store(Arc::new(StsSessionStore::new()))
    }

    /// Construct the service backed by an externally-owned session
    /// store. Use this when other services (Cognito Identity, the
    /// principal-lookup chain) need to see credentials issued by STS.
    pub fn with_session_store(sessions: Arc<StsSessionStore>) -> Self {
        Self {
            trust_policy: OnceLock::new(),
            sessions,
        }
    }

    /// Borrow the shared session store. Used by main.rs to wire the
    /// same store into the principal-lookup chain.
    pub fn session_store(&self) -> &Arc<StsSessionStore> {
        &self.sessions
    }

    /// Wire in the trust-policy resolver. Idempotent: first call wins so the
    /// gateway can install it after IAM is up without racing with itself.
    /// When unset, AssumeRole keeps its old permissive behaviour: real
    /// production deployments should always wire one in, but tests and
    /// embedded uses that don't have IAM state can opt out.
    pub fn set_trust_policy_resolver(&self, resolver: Arc<dyn TrustPolicyResolver>) {
        let _ = self.trust_policy.set(resolver);
    }

    fn trust_policy_resolver(&self) -> Option<&Arc<dyn TrustPolicyResolver>> {
        self.trust_policy.get()
    }

    fn get_caller_identity(&self, ctx: &RequestContext) -> Result<Value, AwsError> {
        debug!(
            account_id = %ctx.account_id,
            access_key = ?ctx.access_key,
            "GetCallerIdentity"
        );
        // Resolution order:
        //   1. Recorded STS session (assumed-role temp creds) — emit
        //      the proper `arn:aws:sts::…:assumed-role/Name/Session`
        //      shape and `AROA…:Session` UserId.
        //   2. Long-term access key — surface as a synthetic IAM-user
        //      ARN. Real AWS would resolve this to the actual user via
        //      the IAM record; we don't have a hook to that here, so
        //      we fall back to using the key as the UserId. Tools that
        //      key off UserId still get a stable per-credential value.
        //   3. Anonymous — root shape, the historical default.
        let (user_id, arn, account) = if let Some(session) = ctx
            .access_key
            .as_deref()
            .and_then(|k| self.sessions.lookup(k))
        {
            let assumed_arn = format!(
                "arn:aws:sts::{}:assumed-role/{}/{}",
                session.account_id, session.role_name, session.session_name
            );
            (session.assumed_role_id, assumed_arn, session.account_id)
        } else {
            match ctx.access_key.as_deref() {
                Some(key) if !key.is_empty() => (
                    key.to_string(),
                    format!("arn:aws:iam::{}:user/{}", ctx.account_id, key),
                    ctx.account_id.clone(),
                ),
                _ => (
                    ctx.account_id.clone(),
                    format!("arn:aws:iam::{}:root", ctx.account_id),
                    ctx.account_id.clone(),
                ),
            }
        };
        Ok(json!({
            "Account": account,
            "Arn": arn,
            "UserId": user_id,
        }))
    }

    fn assume_role(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let session_name = input["RoleSessionName"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleSessionName is required"))?;

        validate_role_arn(role_arn)?;
        validate_role_session_name(session_name)?;
        validate_session_tags(input)?;
        validate_session_policies(input)?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);
        validate_assume_role_duration(duration)?;

        if let Some(resolver) = self.trust_policy_resolver() {
            evaluate_trust_policy(resolver.as_ref(), ctx, role_arn)?;
        }

        debug!(role_arn = %role_arn, session_name = %session_name, "AssumeRole");

        let inline_policy = extract_inline_session_policy(input);
        let policy_arns = extract_session_policy_arns(input);
        let (credentials, assumed_role_user, session) = generate_assumed_role_output(
            role_arn,
            session_name,
            &ctx.account_id,
            duration,
            inline_policy,
            policy_arns,
        );
        self.sessions.record(session);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
        }))
    }

    fn get_session_token(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(43200);
        validate_duration_bounds(duration, 900, 129_600, "DurationSeconds")?;

        debug!(account_id = %ctx.account_id, duration, "GetSessionToken");

        let credentials = generate_credentials(duration);
        Ok(json!({ "Credentials": credentials }))
    }

    fn assume_role_with_web_identity(
        &self,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let session_name = input["RoleSessionName"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleSessionName is required"))?;

        validate_role_arn(role_arn)?;
        validate_role_session_name(session_name)?;

        // WebIdentityToken is required by AWS. We don't verify the JWT
        // signature against any IDP, but we extract the `sub` claim to
        // surface as `SubjectFromWebIdentityToken` so callers that key
        // off the subject (per-user identity) get a stable distinct
        // value per token rather than a hardcoded fixture string.
        let token = input["WebIdentityToken"]
            .as_str()
            .ok_or_else(|| AwsError::validation("WebIdentityToken is required"))?;
        let subject =
            extract_jwt_subject(token).unwrap_or_else(|| "web-identity-subject".to_string());

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);
        validate_assume_role_duration(duration)?;

        debug!(role_arn = %role_arn, session_name = %session_name, "AssumeRoleWithWebIdentity");

        let inline_policy = extract_inline_session_policy(input);
        let policy_arns = extract_session_policy_arns(input);
        let (credentials, assumed_role_user, session) = generate_assumed_role_output(
            role_arn,
            session_name,
            &ctx.account_id,
            duration,
            inline_policy,
            policy_arns,
        );
        self.sessions.record(session);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
            "SubjectFromWebIdentityToken": subject,
            "Audience": "sts.amazonaws.com",
            "Provider": "sts.amazonaws.com",
        }))
    }

    fn assume_root(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let target_principal = input["TargetPrincipal"]
            .as_str()
            .ok_or_else(|| AwsError::validation("TargetPrincipal is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(900);

        debug!(target_principal = %target_principal, "AssumeRoot");

        let credentials = generate_credentials(duration);
        let source_identity = format!("arn:aws:iam::{}:root", ctx.account_id);

        Ok(json!({
            "Credentials": credentials,
            "SourceIdentity": source_identity,
        }))
    }

    fn get_federation_token(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let name = input["Name"]
            .as_str()
            .ok_or_else(|| AwsError::validation("Name is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(43200);
        validate_duration_bounds(duration, 900, 129_600, "DurationSeconds")?;

        debug!(name = %name, duration, "GetFederationToken");

        let credentials = generate_credentials(duration);
        let federated_user_arn = format!("arn:aws:sts::{}:federated-user/{}", ctx.account_id, name);
        let federated_user_id = format!("{}:{}", ctx.account_id, name);

        Ok(json!({
            "Credentials": credentials,
            "FederatedUser": {
                "FederatedUserId": federated_user_id,
                "Arn": federated_user_arn,
            },
            "PackedPolicySize": 0,
        }))
    }

    fn decode_authorization_message(&self, input: &Value) -> Result<Value, AwsError> {
        let encoded = input["EncodedMessage"]
            .as_str()
            .ok_or_else(|| AwsError::validation("EncodedMessage is required"))?;

        if encoded.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidAuthorizationMessageException",
                "EncodedMessage cannot be empty",
            ));
        }

        let decoded = json!({
            "allowed": false,
            "explicitDeny": false,
            "matchedStatements": {"items": []},
            "failures": {"items": []},
            "context": {
                "principal": {
                    "id": "AIDEXAMPLEAWSIM",
                    "arn": "arn:aws:iam::000000000000:user/awsim-user"
                },
                "action": "unknown:Action",
                "resource": "*",
                "conditions": {"items": []}
            }
        });

        Ok(json!({
            "DecodedMessage": decoded.to_string(),
        }))
    }

    fn get_access_key_info(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let key = input["AccessKeyId"]
            .as_str()
            .ok_or_else(|| AwsError::validation("AccessKeyId is required"))?;

        if key.len() < 16 {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "AccessKeyId must be at least 16 characters",
            ));
        }

        Ok(json!({
            "Account": ctx.account_id,
        }))
    }

    fn assume_role_with_saml(
        &self,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;
        validate_role_arn(role_arn)?;

        let _principal_arn = input["PrincipalArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("PrincipalArn is required"))?;

        // SAMLAssertion is required and AWS extracts the NameID from it
        // (Subject + NameQualifier). We don't parse XML / verify the
        // signature, but we pull a NameID from the base64-decoded assertion
        // when one is present so distinct SAML calls produce distinct
        // session subjects.
        let assertion = input["SAMLAssertion"]
            .as_str()
            .ok_or_else(|| AwsError::validation("SAMLAssertion is required"))?;
        let saml_subject =
            extract_saml_nameid(assertion).unwrap_or_else(|| "saml-subject".to_string());

        // Per AWS: RoleSessionName for SAML is derived from the SAML
        // NameID, not from the role ARN. The legacy fallback to the role
        // name keeps existing fixtures working when the assertion has
        // no extractable NameID.
        let session_name = if saml_subject != "saml-subject" {
            sanitize_session_name(&saml_subject)
        } else {
            role_arn
                .split('/')
                .next_back()
                .unwrap_or("SAMLSession")
                .to_string()
        };
        validate_role_session_name(&session_name)?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);
        validate_assume_role_duration(duration)?;

        debug!(role_arn = %role_arn, "AssumeRoleWithSAML");

        let inline_policy = extract_inline_session_policy(input);
        let policy_arns = extract_session_policy_arns(input);
        let (credentials, assumed_role_user, session) = generate_assumed_role_output(
            role_arn,
            &session_name,
            &ctx.account_id,
            duration,
            inline_policy,
            policy_arns,
        );
        self.sessions.record(session);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
            "Issuer": "https://saml.example.com",
            "Audience": "sts.amazonaws.com",
            "NameQualifier": "awsim-saml",
            "SubjectType": "transient",
            "Subject": saml_subject,
        }))
    }
}

impl Default for StsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for StsService {
    fn service_name(&self) -> &str {
        "sts"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsQuery
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match operation {
            "GetCallerIdentity" => self.get_caller_identity(ctx),
            "AssumeRole" => self.assume_role(&input, ctx),
            "GetSessionToken" => self.get_session_token(&input, ctx),
            "AssumeRoleWithWebIdentity" => self.assume_role_with_web_identity(&input, ctx),
            "AssumeRoleWithSAML" => self.assume_role_with_saml(&input, ctx),
            "GetFederationToken" => self.get_federation_token(&input, ctx),
            "DecodeAuthorizationMessage" => self.decode_authorization_message(&input),
            "GetAccessKeyInfo" => self.get_access_key_info(&input, ctx),
            "AssumeRoot" => self.assume_root(&input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

/// Characters allowed in IAM resource names (e.g., role/user/session names)
/// per the published Smithy patterns: `[\w+=,.@-]`. `\w` here means
/// `[A-Za-z0-9_]`.
fn is_iam_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-')
}

/// Validate the AWS Role ARN shape:
/// `arn:aws(-{partition})?:iam::ACCOUNT_ID:role/PATH`
/// — 12-digit account id, role/path segments composed of `[\w+=,.@-]`
/// segments separated by `/`.
fn validate_role_arn(arn: &str) -> Result<(), AwsError> {
    fn invalid(arn: &str) -> AwsError {
        AwsError::validation(format!(
            "1 validation error detected: Value '{arn}' at 'roleArn' \
             failed to satisfy constraint: Member must satisfy regular \
             expression pattern: arn:aws(-[a-z]+)*:iam::\\d{{12}}:role/[\\w+=,.@-]+"
        ))
    }

    let rest = arn.strip_prefix("arn:aws").ok_or_else(|| invalid(arn))?;
    // Optional partition suffix like `-cn` or `-us-gov`.
    let rest = if let Some(stripped) = rest.strip_prefix(':') {
        stripped
    } else if let Some(after_dash) = rest.strip_prefix('-') {
        // Walk through the partition suffix until the next ':'.
        let colon = after_dash.find(':').ok_or_else(|| invalid(arn))?;
        let (partition, after) = after_dash.split_at(colon);
        if partition.is_empty()
            || !partition
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-')
        {
            return Err(invalid(arn));
        }
        // `after` starts with the ':' we found.
        &after[1..]
    } else {
        return Err(invalid(arn));
    };

    let rest = rest.strip_prefix("iam::").ok_or_else(|| invalid(arn))?;
    let (account, rest) = rest.split_once(':').ok_or_else(|| invalid(arn))?;
    if account.len() != 12 || !account.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid(arn));
    }
    let path = rest.strip_prefix("role/").ok_or_else(|| invalid(arn))?;
    if path.is_empty() {
        return Err(invalid(arn));
    }
    // Each '/'-separated segment must use only the IAM-name charset.
    for segment in path.split('/') {
        if segment.is_empty() || !segment.chars().all(is_iam_name_char) {
            return Err(invalid(arn));
        }
    }
    Ok(())
}

/// Validate `RoleSessionName` per Smithy: 2..=64 chars from `[\w+=,.@-]`.
fn validate_role_session_name(name: &str) -> Result<(), AwsError> {
    if !(2..=64).contains(&name.len()) || !name.chars().all(is_iam_name_char) {
        return Err(AwsError::validation(format!(
            "1 validation error detected: Value '{name}' at 'roleSessionName' \
             failed to satisfy constraint: Member must satisfy regular \
             expression pattern: [\\w+=,.@-]{{2,64}}"
        )));
    }
    Ok(())
}

/// Validate the optional session-policy inputs to AssumeRole. AWS:
///   - inline `Policy`: must parse as JSON; combined size with PolicyArns
///     bound to a packed limit (we use 2048 chars as a rough guard)
///   - `PolicyArns`: max 10 entries, each must be a valid policy ARN
///
/// Note: we don't yet apply session policies to credential evaluation —
/// the IAM enforcement engine is opt-in and AssumeRole credentials are
/// generated without consulting them. Validation here just prevents
/// callers from getting silent acceptance of bad input.
fn validate_session_policies(input: &Value) -> Result<(), AwsError> {
    if let Some(policy) = input.get("Policy").and_then(Value::as_str) {
        if policy.len() > 2048 {
            return Err(AwsError::validation(format!(
                "Session Policy must be at most 2048 characters; got {}",
                policy.len()
            )));
        }
        if !policy.is_empty() {
            serde_json::from_str::<Value>(policy)
                .map_err(|_| AwsError::validation("Session Policy must be valid JSON"))?;
        }
    }
    if let Some(arns) = input.get("PolicyArns").and_then(Value::as_array) {
        if arns.len() > 10 {
            return Err(AwsError::validation(format!(
                "PolicyArns supports up to 10 entries; got {}",
                arns.len()
            )));
        }
        for entry in arns {
            let arn = entry
                .get("arn")
                .or_else(|| entry.get("Arn"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AwsError::validation("Each PolicyArns entry must include an 'arn' field")
                })?;
            // Accept either an account-managed ARN (arn:aws:iam::{12 digits}:policy/...)
            // or an AWS-managed ARN (arn:aws:iam::aws:policy/...).
            let is_account_arn = arn.starts_with("arn:aws:iam::")
                && arn.split(':').nth(4).is_some_and(|seg| {
                    seg == "aws" || (seg.len() == 12 && seg.chars().all(|c| c.is_ascii_digit()))
                })
                && arn.contains(":policy/");
            if !is_account_arn {
                return Err(AwsError::validation(format!(
                    "PolicyArns entry '{arn}' is not a valid IAM policy ARN"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the optional `Tags` and `TransitiveTagKeys` inputs to
/// AssumeRole. AWS rejects:
///   - more than 50 tags per session
///   - tag keys outside 1..=128 chars
///   - tag values over 256 chars
///   - duplicate tag keys (case-sensitive)
///   - TransitiveTagKeys that aren't subsets of the supplied tag keys
fn validate_session_tags(input: &Value) -> Result<(), AwsError> {
    let tags = match input.get("Tags").and_then(Value::as_array) {
        Some(arr) => arr,
        None => return Ok(()),
    };
    if tags.len() > 50 {
        return Err(AwsError::validation(format!(
            "Cannot have more than 50 session tags; got {}",
            tags.len()
        )));
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for tag in tags {
        let key = tag
            .get("Key")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::validation("Each Tag entry must include Key and Value"))?;
        let value = tag
            .get("Value")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::validation("Each Tag entry must include Key and Value"))?;
        if key.is_empty() || key.len() > 128 {
            return Err(AwsError::validation(format!(
                "Tag key length {} is outside 1..=128",
                key.len()
            )));
        }
        if value.len() > 256 {
            return Err(AwsError::validation(format!(
                "Tag value length {} exceeds 256",
                value.len()
            )));
        }
        if !seen.insert(key.to_string()) {
            return Err(AwsError::validation(format!(
                "Duplicate session tag key: {key}"
            )));
        }
    }

    if let Some(transitive) = input.get("TransitiveTagKeys").and_then(Value::as_array) {
        for k in transitive {
            let Some(s) = k.as_str() else {
                return Err(AwsError::validation(
                    "TransitiveTagKeys entries must be strings",
                ));
            };
            if !seen.contains(s) {
                return Err(AwsError::validation(format!(
                    "TransitiveTagKeys entry {s} is not present in Tags"
                )));
            }
        }
    }
    Ok(())
}

/// Extract the `sub` claim from a JWT-shaped Web Identity token. We
/// don't verify the signature against any IDP (no JWKS lookup) — the
/// purpose here is to give callers a stable per-token subject identity
/// instead of a hardcoded fixture string. Returns `None` for malformed
/// tokens so the caller can fall back to a default.
fn extract_jwt_subject(token: &str) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("sub").and_then(Value::as_str).map(String::from)
}

/// Extract a SAML NameID from a base64-encoded SAML assertion. We
/// don't verify the signature; we just look for `<NameID>...</NameID>`
/// (any namespace prefix) inside the decoded XML. Returns `None` when
/// no NameID is found so the caller can fall back.
fn extract_saml_nameid(assertion_b64: &str) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    let bytes = STANDARD.decode(assertion_b64).ok()?;
    let xml = std::str::from_utf8(&bytes).ok()?;
    // Crude scan: find `>...NameID>` close-tag and walk back to its
    // opening tag. Avoids a full XML parser dependency for what is a
    // best-effort extraction.
    let close_idx = xml
        .find("</saml:NameID>")
        .or_else(|| xml.find("</saml2:NameID>"))
        .or_else(|| xml.find("</NameID>"))?;
    let prefix = &xml[..close_idx];
    let open_idx = prefix
        .rfind("<saml:NameID")
        .or_else(|| prefix.rfind("<saml2:NameID"))
        .or_else(|| prefix.rfind("<NameID"))?;
    // Skip past the `>` that closes the open tag.
    let after_open = &prefix[open_idx..];
    let gt_idx = after_open.find('>')?;
    let inner = &after_open[gt_idx + 1..];
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Evaluate the trust policy on `role_arn` against the calling principal.
///
/// AWS rules: a role can be assumed only by a principal explicitly named
/// in its trust policy (the `AssumeRolePolicyDocument`). This is a
/// resource-based policy, so we feed it through the same evaluator as
/// any other resource policy with action `sts:AssumeRole` and the role
/// ARN as the resource.
fn evaluate_trust_policy(
    resolver: &dyn TrustPolicyResolver,
    ctx: &RequestContext,
    role_arn: &str,
) -> Result<(), AwsError> {
    let Some(policy_json) = resolver.resolve(&ctx.account_id, role_arn) else {
        return Err(AwsError::access_denied_for(
            "sts:AssumeRole",
            "anonymous",
            role_arn,
        ));
    };

    let policy = awsim_iam_policy::parse(&policy_json).map_err(|e| {
        AwsError::internal(format!(
            "Stored AssumeRolePolicyDocument for {role_arn} is malformed: {e}"
        ))
    })?;

    // Derive the calling principal from the SigV4 access key. Anonymous /
    // unauthenticated callers fall through as the account root, mirroring
    // GetCallerIdentity's existing behaviour.
    let principal_arn = match ctx.access_key.as_deref() {
        Some(k) if !k.is_empty() => format!("arn:aws:iam::{}:user/{}", ctx.account_id, k),
        _ => format!("arn:aws:iam::{}:root", ctx.account_id),
    };

    let context = std::collections::HashMap::new();
    let req = awsim_iam_policy::AuthzRequest {
        principal_arn: &principal_arn,
        principal_account: &ctx.account_id,
        action: "sts:AssumeRole",
        resource_arn: role_arn,
        context: &context,
    };
    let scps: Vec<awsim_iam_policy::PolicyDocument> = Vec::new();
    let identity_policies: Vec<awsim_iam_policy::PolicyDocument> = Vec::new();
    let eval_ctx = awsim_iam_policy::EvalContext {
        identity_policies: &identity_policies,
        permissions_boundary: None,
        resource_policy: Some(&policy),
        scps: &scps,
        session_policy: None,
    };

    match awsim_iam_policy::evaluate(&req, &eval_ctx) {
        awsim_iam_policy::Decision::Allow => Ok(()),
        _ => Err(AwsError::access_denied_for(
            "sts:AssumeRole",
            &principal_arn,
            role_arn,
        )),
    }
}

/// Coerce an arbitrary string into a valid RoleSessionName by replacing
/// disallowed characters with `-` and clamping the length to 64. AWS
/// session-name regex is `[\w+=,.@-]{2,64}`.
fn sanitize_session_name(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| if is_iam_name_char(c) { c } else { '-' })
        .collect();
    if out.len() < 2 {
        out.push_str("--");
    }
    out.truncate(64);
    out
}

/// Generic duration-seconds bounds check used by GetSessionToken /
/// GetFederationToken (which allow 900..=129 600).
fn validate_duration_bounds(
    seconds: u64,
    min: u64,
    max: u64,
    field_name: &str,
) -> Result<(), AwsError> {
    if !(min..=max).contains(&seconds) {
        return Err(AwsError::validation(format!(
            "1 validation error detected: Value '{seconds}' at '{field_name}' failed to satisfy \
             constraint: Member must have value less than or equal to {max} and greater than or \
             equal to {min}"
        )));
    }
    Ok(())
}

/// Validate the DurationSeconds parameter for AssumeRole. AWS allows
/// 900..=43200 seconds (the upper bound is role-specific in real AWS but
/// we apply the global max).
fn validate_assume_role_duration(seconds: u64) -> Result<(), AwsError> {
    if !(900..=43_200).contains(&seconds) {
        return Err(AwsError::validation(format!(
            "1 validation error detected: Value '{seconds}' at 'durationSeconds' \
             failed to satisfy constraint: Member must have value less than or equal to 43200 \
             and greater than or equal to 900"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Credential generation helpers
// ---------------------------------------------------------------------------

/// Generate a fake but realistically-shaped AWS access key ID.
/// Real access keys: ASIA... (temporary) or AKIA... (long-term).
/// Temporary STS credentials always start with ASIA.
fn fake_access_key_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    // Take 16 uppercase hex chars and prefix with ASIA
    let suffix: String = id[..16].to_uppercase();
    format!("ASIA{suffix}")
}

/// Generate a fake 40-character hex secret access key.
fn fake_secret_access_key() -> String {
    let u1 = uuid::Uuid::new_v4().simple().to_string();
    let u2 = uuid::Uuid::new_v4().simple().to_string();
    // Two UUIDs without hyphens give 64 hex chars; take first 40.
    format!("{u1}{u2}")[..40].to_string()
}

/// Generate a fake session token (base64-ish long string).
fn fake_session_token() -> String {
    let parts: Vec<String> = (0..4)
        .map(|_| uuid::Uuid::new_v4().simple().to_string())
        .collect();
    // Produce a long opaque string that looks like a real session token.
    format!(
        "FwoGZXIvYXdzEAwaDAwsim{}//////////wEaD{}Aw{}Q{}",
        parts[0], parts[1], parts[2], parts[3]
    )
}

/// ISO 8601 expiration timestamp (now + duration_seconds).
fn expiration_timestamp(duration_seconds: u64) -> String {
    // Use a simple offset calculation without an external time crate.
    // We know today is 2026-04-21; generate a static offset for testing parity.
    // In practice, for a local emulator, the exact wall-clock time is acceptable.
    // We use std::time::SystemTime to get UNIX epoch seconds.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expiry = now + duration_seconds;
    unix_to_iso8601(expiry)
}

/// Convert a UNIX timestamp to an ISO 8601 UTC string.
/// Implements a minimal conversion without external crates.
fn unix_to_iso8601(secs: u64) -> String {
    // Days since epoch
    let mut remaining = secs;
    let seconds = remaining % 60;
    remaining /= 60;
    let minutes = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    remaining /= 24;

    // Gregorian calendar calculation from days since 1970-01-01
    let (year, month, day) = days_to_ymd(remaining);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Generate a `Credentials` JSON object.
fn generate_credentials(duration_seconds: u64) -> Value {
    json!({
        "AccessKeyId": fake_access_key_id(),
        "SecretAccessKey": fake_secret_access_key(),
        "SessionToken": fake_session_token(),
        "Expiration": expiration_timestamp(duration_seconds),
    })
}

/// Generate credentials + `AssumedRoleUser` for role assumption
/// operations, plus a session record the caller is expected to drop
/// into [`StsSessionStore`] so subsequent signed requests resolve to
/// the assumed-role principal.
fn generate_assumed_role_output(
    role_arn: &str,
    session_name: &str,
    account_id: &str,
    duration_seconds: u64,
    inline_session_policy: Option<String>,
    session_policy_arns: Vec<String>,
) -> (Value, Value, AssumedRoleSession) {
    let role_id_suffix = uuid::Uuid::new_v4().simple().to_string()[..20].to_uppercase();
    let assumed_role_id = format!("AROA{role_id_suffix}:{session_name}");

    let role_name = sessions::role_name_from_arn(role_arn);
    let assumed_role_arn =
        format!("arn:aws:sts::{account_id}:assumed-role/{role_name}/{session_name}");

    let credentials = generate_credentials(duration_seconds);
    let access_key = credentials["AccessKeyId"]
        .as_str()
        .expect("generate_credentials always emits AccessKeyId")
        .to_string();
    let assumed_role_user = json!({
        "AssumedRoleId": assumed_role_id,
        "Arn": assumed_role_arn,
    });

    let session = AssumedRoleSession {
        access_key,
        role_arn: role_arn.to_string(),
        role_name,
        session_name: session_name.to_string(),
        account_id: account_id.to_string(),
        assumed_role_id,
        expiry: AssumedRoleSession::expiry_from_duration(duration_seconds),
        inline_session_policy,
        session_policy_arns,
    };

    (credentials, assumed_role_user, session)
}

/// Pull the inline `Policy` parameter off an AssumeRole-shaped input.
/// Validation has already run by the time this is called; we just
/// hand back the raw document so the session can carry it through to
/// authz. Returns `None` when the caller didn't pass `Policy`.
fn extract_inline_session_policy(input: &Value) -> Option<String> {
    input
        .get("Policy")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Pull up to 10 managed-policy ARNs off the `PolicyArns` parameter on
/// an AssumeRole-shaped input. AWS rejects more than 10 entries
/// during validation, so we just collect whatever made it through.
fn extract_session_policy_arns(input: &Value) -> Vec<String> {
    input
        .get("PolicyArns")
        .and_then(Value::as_array)
        .map(|arns| {
            arns.iter()
                .filter_map(|item| item.get("arn").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::RequestContext;

    fn make_ctx() -> RequestContext {
        RequestContext::new("sts", "us-east-1")
    }

    #[test]
    fn test_fake_access_key_id_format() {
        let key = fake_access_key_id();
        assert!(key.starts_with("ASIA"), "must start with ASIA: {key}");
        assert_eq!(key.len(), 20, "must be 20 chars: {key}");
    }

    #[test]
    fn test_fake_secret_access_key_length() {
        let secret = fake_secret_access_key();
        assert_eq!(secret.len(), 40, "must be 40 chars: {secret}");
    }

    #[test]
    fn test_get_caller_identity_anonymous_returns_root() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let result = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(result["Account"], "000000000000");
        // Anonymous/unauthenticated → root shape; UserId equals account id.
        assert_eq!(result["UserId"], json!("000000000000"));
        assert_eq!(
            result["Arn"].as_str().unwrap(),
            "arn:aws:iam::000000000000:root"
        );
    }

    #[test]
    fn test_get_caller_identity_uses_access_key_when_present() {
        let svc = StsService::new();
        let mut ctx = make_ctx();
        ctx.access_key = Some("AKIATESTAKID000000".to_string());
        let result = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(result["UserId"], json!("AKIATESTAKID000000"));
        assert_eq!(
            result["Arn"].as_str().unwrap(),
            "arn:aws:iam::000000000000:user/AKIATESTAKID000000"
        );
    }

    #[test]
    fn test_get_caller_identity_assumed_role_uses_session() {
        // Round-trip: AssumeRole records a session, then a follow-up
        // GetCallerIdentity with the issued ASIA key should report the
        // assumed-role ARN and AROA…:session UserId — not the
        // synthesised iam:user/ASIA… shape.
        let svc = StsService::new();
        let assume_input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/AppAuthRole",
            "RoleSessionName": "app-session",
        });
        let assume_out = svc.assume_role(&assume_input, &make_ctx()).unwrap();
        let asia = assume_out["Credentials"]["AccessKeyId"]
            .as_str()
            .unwrap()
            .to_string();

        let mut ctx = make_ctx();
        ctx.access_key = Some(asia);
        let id = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(
            id["Arn"].as_str().unwrap(),
            "arn:aws:sts::000000000000:assumed-role/AppAuthRole/app-session"
        );
        assert!(id["UserId"].as_str().unwrap().starts_with("AROA"));
        assert!(id["UserId"].as_str().unwrap().ends_with(":app-session"));
    }

    #[test]
    fn test_assume_role_missing_role_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({ "RoleSessionName": "my-session" });
        let err = svc.assume_role(&input, &ctx).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_missing_session_name() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({ "RoleArn": "arn:aws:iam::000000000000:role/MyRole" });
        let err = svc.assume_role(&input, &ctx).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
            "RoleSessionName": "test-session",
        });
        let result = svc.assume_role(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretAccessKey"].as_str().unwrap().len(), 40);
        assert!(!creds["SessionToken"].as_str().unwrap().is_empty());
        assert!(!creds["Expiration"].as_str().unwrap().is_empty());

        let aru = &result["AssumedRoleUser"];
        assert!(
            aru["Arn"]
                .as_str()
                .unwrap()
                .contains("assumed-role/MyRole/test-session")
        );
        assert!(
            aru["AssumedRoleId"]
                .as_str()
                .unwrap()
                .contains("test-session")
        );
    }

    #[test]
    fn test_assume_role_rejects_malformed_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "not-an-arn",
                    "RoleSessionName": "session",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("roleArn"));
    }

    #[test]
    fn test_assume_role_rejects_session_name_too_short() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "x",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("roleSessionName"));
    }

    #[test]
    fn test_assume_role_rejects_session_name_with_invalid_chars() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "bad name with spaces",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_partition_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws-us-gov:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_accepts_role_with_path() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/team/dev/MyRole",
                "RoleSessionName": "session",
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_with_web_identity_extracts_jwt_sub_claim() {
        use base64::Engine as _;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
        let svc = StsService::new();
        let ctx = make_ctx();
        // Hand-crafted JWT: header.payload.signature; payload has `sub`.
        let header = B64URL.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = B64URL.encode(br#"{"sub":"user@idp.example","aud":"awsim"}"#);
        let token = format!("{header}.{payload}.signature");

        let resp = svc
            .assume_role_with_web_identity(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "WebIdentityToken": token,
                }),
                &ctx,
            )
            .unwrap();
        assert_eq!(
            resp["SubjectFromWebIdentityToken"].as_str(),
            Some("user@idp.example")
        );
    }

    #[test]
    fn test_assume_role_with_saml_extracts_nameid() {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as B64;
        let svc = StsService::new();
        let ctx = make_ctx();
        let assertion_xml = r#"<saml:Assertion><saml:Subject><saml:NameID>alice@example.com</saml:NameID></saml:Subject></saml:Assertion>"#;
        let assertion = B64.encode(assertion_xml);

        let resp = svc
            .assume_role_with_saml(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "PrincipalArn": "arn:aws:iam::000000000000:saml-provider/idp",
                    "SAMLAssertion": assertion,
                }),
                &ctx,
            )
            .unwrap();
        assert_eq!(resp["Subject"].as_str(), Some("alice@example.com"));
        // Session name was derived from the NameID (sanitized — `@` and
        // `.` are valid IAM-name chars so they pass through).
        let session_arn = resp["AssumedRoleUser"]["Arn"].as_str().unwrap();
        assert!(
            session_arn.contains("alice@example.com"),
            "arn={session_arn}"
        );
    }

    #[test]
    fn test_get_session_token_rejects_duration_above_129600() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .get_session_token(&json!({ "DurationSeconds": 200_000u64 }), &ctx)
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_get_session_token_rejects_duration_below_900() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .get_session_token(&json!({ "DurationSeconds": 60u64 }), &ctx)
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_get_federation_token_enforces_duration_bounds() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .get_federation_token(
                &json!({ "Name": "fed", "DurationSeconds": 200_000u64 }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_rejects_invalid_policy_json() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Policy": "{not-json",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_rejects_too_many_policy_arns() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let arns: Vec<Value> = (0..11)
            .map(|i| json!({ "arn": format!("arn:aws:iam::aws:policy/P{i}") }))
            .collect();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "PolicyArns": arns,
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_rejects_malformed_policy_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "PolicyArns": [{ "arn": "not-an-arn" }],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_valid_session_policies() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
                "Policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
                "PolicyArns": [
                    { "arn": "arn:aws:iam::aws:policy/AdministratorAccess" },
                    { "arn": "arn:aws:iam::000000000000:policy/Custom" },
                ],
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_rejects_duplicate_tag_keys() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Tags": [
                        { "Key": "team", "Value": "infra" },
                        { "Key": "team", "Value": "data" },
                    ],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("Duplicate"));
    }

    #[test]
    fn test_assume_role_rejects_transitive_tag_not_in_tags() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Tags": [{ "Key": "team", "Value": "infra" }],
                    "TransitiveTagKeys": ["nonexistent"],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_well_formed_tags() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
                "Tags": [
                    { "Key": "team", "Value": "infra" },
                    { "Key": "env", "Value": "prod" },
                ],
                "TransitiveTagKeys": ["team"],
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_rejects_duration_below_900() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "DurationSeconds": 60u64,
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("durationSeconds"));
    }

    #[test]
    fn test_get_session_token_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({});
        let result = svc.get_session_token(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
    }

    #[test]
    fn test_assume_role_with_web_identity_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/WebRole",
            "RoleSessionName": "web-session",
            "WebIdentityToken": "fake-oidc-token",
        });
        let result = svc.assume_role_with_web_identity(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
    }

    #[test]
    fn test_assume_role_with_saml_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/SAMLRole",
            "PrincipalArn": "arn:aws:iam::000000000000:saml-provider/MyProvider",
            "SAMLAssertion": "base64-encoded-saml-assertion",
        });
        let result = svc.assume_role_with_saml(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        let aru = &result["AssumedRoleUser"];
        assert!(
            aru["Arn"]
                .as_str()
                .unwrap()
                .contains("assumed-role/SAMLRole")
        );
    }

    #[test]
    fn test_unknown_operation() {
        // The handle() dispatch is synchronous for the unknown-operation branch,
        // so we can call the internal match directly via a blocking future executor.
        let svc = StsService::new();
        let ctx = make_ctx();
        // Drive the async fn to completion with a minimal single-threaded executor.
        let fut = svc.handle("NonExistentOp", json!({}), &ctx);
        let err = futures_executor_block_on(fut).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    /// Minimal blocking executor that drives a future to completion on the current thread.
    fn futures_executor_block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }

        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn test_expiration_timestamp_format() {
        let ts = expiration_timestamp(3600);
        // Must match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "must end with Z: {ts}");
        assert_eq!(ts.len(), 20, "must be 20 chars: {ts}");
    }

    struct StaticResolver(Option<&'static str>);

    impl TrustPolicyResolver for StaticResolver {
        fn resolve(&self, _account_id: &str, _role_arn: &str) -> Option<String> {
            self.0.map(String::from)
        }
    }

    #[test]
    fn assume_role_rejected_when_role_not_found() {
        let svc = StsService::new();
        svc.set_trust_policy_resolver(Arc::new(StaticResolver(None)));
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/Missing",
                    "RoleSessionName": "session",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn assume_role_rejected_when_trust_policy_does_not_allow_caller() {
        let svc = StsService::new();
        // Trust policy only allows a different account. Our caller is anonymous
        // (root of 000000000000), so evaluation must implicit-deny.
        svc.set_trust_policy_resolver(Arc::new(StaticResolver(Some(
            r#"{
                "Version":"2012-10-17",
                "Statement":[{
                    "Effect":"Allow",
                    "Principal":{"AWS":"arn:aws:iam::999999999999:root"},
                    "Action":"sts:AssumeRole"
                }]
            }"#,
        ))));
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/Locked",
                    "RoleSessionName": "session",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "AccessDenied");
    }

    #[test]
    fn assume_role_succeeds_when_trust_policy_explicitly_allows_caller() {
        let svc = StsService::new();
        svc.set_trust_policy_resolver(Arc::new(StaticResolver(Some(
            r#"{
                "Version":"2012-10-17",
                "Statement":[{
                    "Effect":"Allow",
                    "Principal":{"AWS":"arn:aws:iam::000000000000:root"},
                    "Action":"sts:AssumeRole"
                }]
            }"#,
        ))));
        let ctx = make_ctx();
        let resp = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/Open",
                    "RoleSessionName": "session",
                }),
                &ctx,
            )
            .unwrap();
        assert!(resp["Credentials"]["AccessKeyId"].as_str().is_some());
    }
}
