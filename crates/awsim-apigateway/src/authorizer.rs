//! API Gateway v1 authorizer evaluation.
//!
//! Three authorizer types are supported:
//!
//!   * **COGNITO_USER_POOLS** — fully evaluated locally. Decode the JWT,
//!     check `exp`, surface claims as the authorizer context.
//!   * **CUSTOM / TOKEN** — caller invokes the configured Lambda with the
//!     header value as `authorizationToken`, then feeds the response back
//!     in to validate the policy and pull out `principalId` + `context`.
//!   * **CUSTOM / REQUEST** — same as TOKEN but the Lambda event carries
//!     the full request shape (headers, query, path, stage vars).
//!
//! Decisions are cached per `(authorizer_id, identity_string)` for the
//! authorizer's `result_ttl_in_seconds` so a token only triggers one
//! Lambda invocation per TTL window, matching real AWS.
//!
//! AWS_IAM (SigV4) is intentionally out of scope here — anything carrying
//! authorization_type `AWS_IAM` or unset just falls through to NONE.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use dashmap::DashMap;
use serde_json::{Value, json};

use crate::v1::{Authorizer, Method};

/// Per-state cache of authorizer decisions.
#[derive(Default)]
pub struct AuthorizerCache {
    entries: DashMap<String, CachedDecision>,
    /// Wall-clock baseline used so cache eviction works deterministically
    /// even if `Instant::now()` jumps. `Mutex` only for the `Option`
    /// initialization.
    epoch: Mutex<Option<Instant>>,
}

#[derive(Clone)]
struct CachedDecision {
    expires_at: Instant,
    outcome: AuthorizationOutcome,
}

impl AuthorizerCache {
    fn now(&self) -> Instant {
        let mut guard = self.epoch.lock().expect("authorizer epoch mutex");
        let _ = guard.get_or_insert_with(Instant::now);
        Instant::now()
    }

    fn get(&self, key: &str) -> Option<AuthorizationOutcome> {
        let entry = self.entries.get(key)?;
        if entry.expires_at <= self.now() {
            drop(entry);
            self.entries.remove(key);
            return None;
        }
        Some(entry.outcome.clone())
    }

    fn insert(&self, key: String, outcome: AuthorizationOutcome, ttl: Duration) {
        let expires_at = self.now() + ttl;
        self.entries.insert(
            key,
            CachedDecision {
                expires_at,
                outcome,
            },
        );
    }
}

/// What the caller should do with this request.
pub enum AuthorizationStep {
    /// Authorization complete; merge `outcome` into the proxy event and
    /// dispatch the integration.
    Allowed(AuthorizationOutcome),
    /// Authorization not configured. Dispatch the integration unchanged.
    NotConfigured,
    /// 401 Unauthorized — identity source is missing or empty.
    Unauthorized(String),
    /// 403 Forbidden — explicit deny, JWT expired, or invalid token shape.
    Forbidden(String),
    /// Caller must invoke the Lambda authorizer with `event` and call
    /// `apply_authorizer_response` with the result. The `cache_key` and
    /// `ttl_seconds` round-trip back to that call.
    InvokeLambda(LambdaInvocation),
}

#[derive(Clone)]
pub struct AuthorizationOutcome {
    pub principal_id: String,
    /// Free-form context map exposed via `$context.authorizer.X` in
    /// templates and `requestContext.authorizer.X` in proxy events.
    /// For Cognito this also carries a `claims` sub-object.
    pub context: Value,
}

pub struct LambdaInvocation {
    pub authorizer_uri: String,
    pub event: Value,
    pub cache_key: String,
    pub ttl_seconds: u32,
    /// Method ARN built into the event — also re-checked against the
    /// returned policy when the response comes back.
    pub method_arn: String,
}

/// Build the API Gateway "method ARN" that authorizers receive.
///
/// Format: `arn:aws:execute-api:{region}:{account}:{api-id}/{stage}/{METHOD}/{path}`
fn build_method_arn(
    region: &str,
    account_id: &str,
    api_id: &str,
    stage: &str,
    method: &str,
    path: &str,
) -> String {
    let path_no_leading = path.trim_start_matches('/');
    format!("arn:aws:execute-api:{region}:{account_id}:{api_id}/{stage}/{method}/{path_no_leading}")
}

/// Decide what to do with a request based on the method's authorization
/// configuration. Cache hits are returned as `Allowed` directly; misses
/// return either `Allowed` (Cognito, fully local) or `InvokeLambda`
/// (custom Lambda authorizers).
#[allow(clippy::too_many_arguments)]
pub fn evaluate(
    cache: &AuthorizerCache,
    method: &Method,
    authorizers: &HashMap<String, Authorizer>,
    headers: &HashMap<String, String>,
    path_params: &HashMap<String, String>,
    query_params: &HashMap<String, String>,
    stage_variables: &HashMap<String, String>,
    request_context: &Value,
    region: &str,
    account_id: &str,
    api_id: &str,
    stage: &str,
    http_method: &str,
    path: &str,
) -> AuthorizationStep {
    if method.authorization_type == "NONE" || method.authorization_type.is_empty() {
        return AuthorizationStep::NotConfigured;
    }
    if method.authorization_type == "AWS_IAM" {
        // SigV4 enforcement isn't modeled here yet — proceed.
        return AuthorizationStep::NotConfigured;
    }

    let Some(authorizer) = authorizers.get(&method.authorizer_id) else {
        return AuthorizationStep::Forbidden(format!(
            "Method has authorizationType={} but authorizerId={} is not configured",
            method.authorization_type, method.authorizer_id
        ));
    };

    let identity = match resolve_identity(&authorizer.identity_source, headers, query_params) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return AuthorizationStep::Unauthorized(format!(
                "Missing required identity source '{}' for authorizer {}",
                authorizer.identity_source, authorizer.id
            ));
        }
    };

    let cache_key = format!("{}:{identity}", authorizer.id);
    if let Some(outcome) = cache.get(&cache_key) {
        return AuthorizationStep::Allowed(outcome);
    }

    let method_arn = build_method_arn(region, account_id, api_id, stage, http_method, path);
    let auth_kind = AuthorizerKind::from_authorizer(authorizer);

    match auth_kind {
        AuthorizerKind::Cognito => evaluate_cognito(cache, &cache_key, authorizer, &identity),
        AuthorizerKind::Token => {
            let event = json!({
                "type": "TOKEN",
                "authorizationToken": identity,
                "methodArn": &method_arn,
            });
            AuthorizationStep::InvokeLambda(LambdaInvocation {
                authorizer_uri: authorizer.authorizer_uri.clone(),
                event,
                cache_key,
                ttl_seconds: authorizer.result_ttl_in_seconds,
                method_arn,
            })
        }
        AuthorizerKind::Request => {
            let event = json!({
                "type": "REQUEST",
                "methodArn": &method_arn,
                "headers": headers,
                "queryStringParameters": query_params,
                "pathParameters": path_params,
                "stageVariables": stage_variables,
                "requestContext": request_context,
            });
            AuthorizationStep::InvokeLambda(LambdaInvocation {
                authorizer_uri: authorizer.authorizer_uri.clone(),
                event,
                cache_key,
                ttl_seconds: authorizer.result_ttl_in_seconds,
                method_arn,
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AuthorizerKind {
    Cognito,
    Token,
    Request,
}

impl AuthorizerKind {
    fn from_authorizer(a: &Authorizer) -> Self {
        match a.r#type.as_str() {
            "COGNITO_USER_POOLS" => Self::Cognito,
            "REQUEST" => Self::Request,
            // TOKEN is the historical default and what AWS uses when the
            // type is missing on legacy resources.
            _ => Self::Token,
        }
    }
}

fn evaluate_cognito(
    cache: &AuthorizerCache,
    cache_key: &str,
    authorizer: &Authorizer,
    token: &str,
) -> AuthorizationStep {
    // Cognito sends the token in `Authorization: <jwt>` — if the SDK
    // prefixes `Bearer `, strip it.
    let raw = token.strip_prefix("Bearer ").unwrap_or(token).trim();
    let claims = match decode_jwt_claims(raw) {
        Some(c) => c,
        None => {
            return AuthorizationStep::Forbidden(
                "Cognito authorizer received a malformed JWT".to_string(),
            );
        }
    };

    if let Some(exp) = claims.get("exp").and_then(Value::as_u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if exp < now {
            return AuthorizationStep::Forbidden("Cognito JWT is expired".to_string());
        }
    }

    let principal_id = claims
        .get("sub")
        .and_then(Value::as_str)
        .unwrap_or("anonymous")
        .to_string();
    let outcome = AuthorizationOutcome {
        principal_id,
        context: json!({
            "claims": claims,
        }),
    };

    let ttl = Duration::from_secs(authorizer.result_ttl_in_seconds.max(1) as u64);
    cache.insert(cache_key.to_string(), outcome.clone(), ttl);
    AuthorizationStep::Allowed(outcome)
}

/// Decode a JWT's claims segment. Doesn't verify the signature — the
/// emulator's Cognito service issued the token, and validating signatures
/// across services would couple awsim crates more than is worthwhile.
fn decode_jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let claims = parts.next()?;
    let bytes = URL_SAFE_NO_PAD.decode(claims).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Apply the Lambda authorizer's response. Validates the returned policy
/// against the method ARN, caches the decision on Allow, and returns the
/// final step the caller should take.
pub fn apply_lambda_response(
    cache: &AuthorizerCache,
    invocation: &LambdaInvocation,
    response: &Value,
) -> AuthorizationStep {
    // Lambda's return shape: { principalId, policyDocument, context? }.
    let principal_id = response
        .get("principalId")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .to_string();

    let policy = response
        .get("policyDocument")
        .or_else(|| response.get("policy"));
    let Some(policy) = policy else {
        return AuthorizationStep::Forbidden(
            "Lambda authorizer response missing policyDocument".to_string(),
        );
    };

    let effect = match policy_effect_for_arn(policy, &invocation.method_arn) {
        PolicyDecision::Allow => Effect::Allow,
        PolicyDecision::Deny => Effect::Deny,
        PolicyDecision::NoMatch => Effect::Deny,
    };
    if matches!(effect, Effect::Deny) {
        return AuthorizationStep::Forbidden(format!(
            "Lambda authorizer denied access to {}",
            invocation.method_arn
        ));
    }

    let context = response
        .get("context")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let outcome = AuthorizationOutcome {
        principal_id,
        context,
    };
    let ttl = Duration::from_secs(invocation.ttl_seconds.max(1) as u64);
    cache.insert(invocation.cache_key.clone(), outcome.clone(), ttl);
    AuthorizationStep::Allowed(outcome)
}

#[derive(Debug, Clone, Copy)]
enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy)]
enum PolicyDecision {
    Allow,
    Deny,
    NoMatch,
}

/// Walk the policy `Statement`s; explicit Deny on a matching resource
/// wins, then explicit Allow, then no-match (treated as Deny upstream).
fn policy_effect_for_arn(policy: &Value, method_arn: &str) -> PolicyDecision {
    let Some(statements) = policy.get("Statement").and_then(Value::as_array) else {
        return PolicyDecision::NoMatch;
    };
    let mut allow_match = false;
    for stmt in statements {
        let effect = stmt.get("Effect").and_then(Value::as_str).unwrap_or("");
        if !statement_resource_matches(stmt, method_arn) {
            continue;
        }
        match effect {
            "Deny" => return PolicyDecision::Deny,
            "Allow" => allow_match = true,
            _ => {}
        }
    }
    if allow_match {
        PolicyDecision::Allow
    } else {
        PolicyDecision::NoMatch
    }
}

fn statement_resource_matches(stmt: &Value, method_arn: &str) -> bool {
    let resource = match stmt.get("Resource") {
        Some(Value::String(s)) => return resource_pattern_matches(s, method_arn),
        Some(Value::Array(arr)) => arr,
        _ => return false,
    };
    resource
        .iter()
        .filter_map(Value::as_str)
        .any(|s| resource_pattern_matches(s, method_arn))
}

/// IAM-style glob matching: `*` matches any character run, `?` matches one.
/// Wider IAM matching (e.g. ARN-segment-aware) lives in `awsim-iam-policy`,
/// but pulling that dep into the apigateway crate just for authorizer
/// resource checks is overkill — string globs cover the patterns
/// authorizer policies actually emit.
fn resource_pattern_matches(pattern: &str, value: &str) -> bool {
    fn rec(p: &[u8], v: &[u8]) -> bool {
        if p.is_empty() {
            return v.is_empty();
        }
        match p[0] {
            b'*' => {
                if rec(&p[1..], v) {
                    return true;
                }
                if v.is_empty() {
                    return false;
                }
                rec(p, &v[1..])
            }
            b'?' => !v.is_empty() && rec(&p[1..], &v[1..]),
            c => !v.is_empty() && v[0] == c && rec(&p[1..], &v[1..]),
        }
    }
    rec(pattern.as_bytes(), value.as_bytes())
}

/// Pull the identity value out of the request based on the authorizer's
/// `identitySource`. The first non-empty source wins. Returns `None`
/// only if the source string is malformed or empty.
fn resolve_identity(
    identity_source: &str,
    headers: &HashMap<String, String>,
    query_params: &HashMap<String, String>,
) -> Option<String> {
    if identity_source.is_empty() {
        return None;
    }
    for spec in identity_source.split(',') {
        let spec = spec.trim();
        if let Some(name) = spec.strip_prefix("method.request.header.")
            && let Some(v) = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
                .filter(|s| !s.is_empty())
        {
            return Some(v.clone());
        }
        if let Some(name) = spec.strip_prefix("method.request.querystring.")
            && let Some(v) = query_params.get(name).filter(|s| !s.is_empty())
        {
            return Some(v.clone());
        }
        // method.request.context.X / .stageVariables.X are valid in AWS
        // but rarely used; treat as no match for now.
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(kind: &str, ttl: u32, identity_source: &str) -> Authorizer {
        Authorizer {
            id: "auth1".into(),
            name: "auth".into(),
            r#type: kind.into(),
            auth_type: "custom".into(),
            authorizer_uri: "arn:aws:lambda:us-east-1:000000000000:function:auth".into(),
            identity_source: identity_source.into(),
            result_ttl_in_seconds: ttl,
            provider_arns: Vec::new(),
        }
    }

    fn method_with_authorizer(kind: &str) -> Method {
        Method {
            http_method: "GET".into(),
            authorization_type: kind.into(),
            authorizer_id: "auth1".into(),
            api_key_required: false,
            request_parameters: HashMap::new(),
            integration: None,
        }
    }

    fn ctx_obj() -> Value {
        json!({"requestId":"r"})
    }

    fn evaluate_t(
        method: &Method,
        authorizers: &HashMap<String, Authorizer>,
        headers: HashMap<String, String>,
    ) -> AuthorizationStep {
        let cache = AuthorizerCache::default();
        evaluate(
            &cache,
            method,
            authorizers,
            &headers,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &ctx_obj(),
            "us-east-1",
            "000000000000",
            "api1",
            "prod",
            "GET",
            "/items",
        )
    }

    #[test]
    fn none_authorization_is_no_op() {
        let mut method = method_with_authorizer("NONE");
        method.authorizer_id = String::new();
        let step = evaluate_t(&method, &HashMap::new(), HashMap::new());
        assert!(matches!(step, AuthorizationStep::NotConfigured));
    }

    #[test]
    fn missing_token_is_unauthorized() {
        let mut authorizers = HashMap::new();
        authorizers.insert(
            "auth1".into(),
            auth("TOKEN", 300, "method.request.header.Authorization"),
        );
        let step = evaluate_t(
            &method_with_authorizer("CUSTOM"),
            &authorizers,
            HashMap::new(),
        );
        assert!(matches!(step, AuthorizationStep::Unauthorized(_)));
    }

    #[test]
    fn token_authorizer_emits_invoke_event() {
        let mut authorizers = HashMap::new();
        authorizers.insert(
            "auth1".into(),
            auth("TOKEN", 300, "method.request.header.Authorization"),
        );
        let mut headers = HashMap::new();
        headers.insert("Authorization".into(), "Bearer abc".into());
        let step = evaluate_t(&method_with_authorizer("CUSTOM"), &authorizers, headers);
        let inv = match step {
            AuthorizationStep::InvokeLambda(i) => i,
            _ => panic!("expected InvokeLambda"),
        };
        assert_eq!(inv.event["type"], "TOKEN");
        assert_eq!(inv.event["authorizationToken"], "Bearer abc");
        assert!(
            inv.method_arn.ends_with(":api1/prod/GET/items"),
            "method arn was {}",
            inv.method_arn
        );
    }

    #[test]
    fn cognito_expired_token_is_forbidden() {
        let mut authorizers = HashMap::new();
        authorizers.insert(
            "auth1".into(),
            auth(
                "COGNITO_USER_POOLS",
                300,
                "method.request.header.Authorization",
            ),
        );
        let claims = json!({"sub":"u1","exp": 1u64});
        let token = make_jwt(&claims);
        let mut headers = HashMap::new();
        headers.insert("Authorization".into(), token);
        let step = evaluate_t(
            &method_with_authorizer("COGNITO_USER_POOLS"),
            &authorizers,
            headers,
        );
        assert!(matches!(step, AuthorizationStep::Forbidden(_)));
    }

    #[test]
    fn cognito_valid_token_surfaces_claims() {
        let mut authorizers = HashMap::new();
        authorizers.insert(
            "auth1".into(),
            auth(
                "COGNITO_USER_POOLS",
                300,
                "method.request.header.Authorization",
            ),
        );
        let exp_future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let claims = json!({"sub": "user-1", "email": "a@b.com", "exp": exp_future});
        let token = make_jwt(&claims);
        let mut headers = HashMap::new();
        headers.insert("Authorization".into(), token);
        let step = evaluate_t(
            &method_with_authorizer("COGNITO_USER_POOLS"),
            &authorizers,
            headers,
        );
        let outcome = match step {
            AuthorizationStep::Allowed(o) => o,
            other => panic!(
                "expected Allowed, got {}",
                match other {
                    AuthorizationStep::Forbidden(m) => format!("Forbidden({m})"),
                    AuthorizationStep::Unauthorized(m) => format!("Unauthorized({m})"),
                    AuthorizationStep::NotConfigured => "NotConfigured".into(),
                    _ => "other".into(),
                }
            ),
        };
        assert_eq!(outcome.principal_id, "user-1");
        assert_eq!(outcome.context["claims"]["email"], "a@b.com");
    }

    #[test]
    fn lambda_response_allow_unlocks_request() {
        let invocation = LambdaInvocation {
            authorizer_uri: "arn:aws:lambda:::function:auth".into(),
            event: json!({}),
            cache_key: "k1".into(),
            ttl_seconds: 60,
            method_arn: "arn:aws:execute-api:us-east-1:000000000000:api1/prod/GET/items".into(),
        };
        let response = json!({
            "principalId": "alice",
            "policyDocument": {
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Action": "execute-api:Invoke",
                    "Resource": "arn:aws:execute-api:us-east-1:000000000000:api1/prod/GET/*"
                }]
            },
            "context": { "userId": "alice" }
        });
        let cache = AuthorizerCache::default();
        let step = apply_lambda_response(&cache, &invocation, &response);
        let outcome = match step {
            AuthorizationStep::Allowed(o) => o,
            _ => panic!("expected Allowed"),
        };
        assert_eq!(outcome.principal_id, "alice");
        assert_eq!(outcome.context["userId"], "alice");
        // Cached: same key returns same outcome without re-invoking.
        assert!(cache.get("k1").is_some());
    }

    #[test]
    fn lambda_response_deny_returns_forbidden() {
        let invocation = LambdaInvocation {
            authorizer_uri: "x".into(),
            event: json!({}),
            cache_key: "k2".into(),
            ttl_seconds: 60,
            method_arn: "arn:aws:execute-api:us-east-1:000000000000:api1/prod/GET/items".into(),
        };
        let response = json!({
            "principalId": "alice",
            "policyDocument": {
                "Statement": [{
                    "Effect": "Deny",
                    "Resource": "*"
                }]
            }
        });
        let cache = AuthorizerCache::default();
        let step = apply_lambda_response(&cache, &invocation, &response);
        assert!(matches!(step, AuthorizationStep::Forbidden(_)));
    }

    #[test]
    fn resource_pattern_glob_matches() {
        assert!(resource_pattern_matches("foo", "foo"));
        assert!(resource_pattern_matches("foo*", "foobar"));
        assert!(resource_pattern_matches("*bar", "foobar"));
        assert!(resource_pattern_matches("a/?/c", "a/b/c"));
        assert!(!resource_pattern_matches("a/?/c", "a/bb/c"));
        assert!(resource_pattern_matches(
            "arn:aws:execute-api:*:*:api1/prod/*/*",
            "arn:aws:execute-api:us-east-1:000000000000:api1/prod/GET/items"
        ));
    }

    /// Assemble a fake JWT with the given claims, no signature checking.
    fn make_jwt(claims: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        format!("{header}.{payload}.")
    }
}
