use crate::document::{
    BaseOperator, Condition, ConditionBlock, Effect, PolicyDocument, Principal, SetQualifier,
    Statement,
};
use crate::glob;
use chrono::{DateTime, TimeZone, Utc};
use ipnet::IpNet;
use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum ContextValue {
    String(String),
    StringList(Vec<String>),
    Number(f64),
    Bool(bool),
    Date(DateTime<Utc>),
    Ip(String),
}

impl ContextValue {
    fn as_strings(&self) -> Vec<String> {
        match self {
            ContextValue::String(s) => vec![s.clone()],
            ContextValue::StringList(v) => v.clone(),
            ContextValue::Number(n) => vec![format_number(*n)],
            ContextValue::Bool(b) => vec![b.to_string()],
            ContextValue::Date(d) => vec![d.to_rfc3339()],
            ContextValue::Ip(s) => vec![s.clone()],
        }
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

pub struct AuthzRequest<'a> {
    pub principal_arn: &'a str,
    pub principal_account: &'a str,
    pub action: &'a str,
    pub resource_arn: &'a str,
    pub context: &'a HashMap<String, ContextValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    ExplicitDeny,
    ImplicitDeny,
}

#[derive(Default)]
pub struct EvalContext<'a> {
    pub identity_policies: &'a [PolicyDocument],
    pub permissions_boundary: Option<&'a PolicyDocument>,
    pub resource_policy: Option<&'a PolicyDocument>,
    pub scps: &'a [PolicyDocument],
    pub session_policy: Option<&'a PolicyDocument>,
}

/// Where a policy came from, for the simulator's `MatchedStatements`
/// output. AWS uses string sentinels like "IAM Policy", "Resource",
/// "User", "Group"; we use a typed enum and stringify at the API
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicySource {
    /// Identity-based: user inline, group inline, attached managed.
    Identity,
    PermissionsBoundary,
    Resource,
    Scp,
    Session,
}

/// Provenance for a single policy passed into the evaluator.
#[derive(Debug, Clone)]
pub struct PolicyAttribution {
    pub source_id: String,
    pub source_type: PolicySource,
}

/// Slice of attribution metadata aligned by index with the
/// corresponding policies in [`EvalContext`]. Pass empty slices /
/// `None` when you don't need matched-statement reporting (e.g. the
/// hot-path authz check).
#[derive(Default, Clone)]
pub struct PolicyAttributions<'a> {
    pub identity: &'a [PolicyAttribution],
    pub permissions_boundary: Option<&'a PolicyAttribution>,
    pub resource: Option<&'a PolicyAttribution>,
    pub scps: &'a [PolicyAttribution],
    pub session: Option<&'a PolicyAttribution>,
}

/// One row in the simulator's `MatchedStatements` array — a statement
/// from one of the input policies whose `Action` + `Resource` (+
/// `Principal`, for resource policies) + `Condition` all matched the
/// simulated request.
#[derive(Debug, Clone)]
pub struct MatchedStatement {
    pub source_id: String,
    pub source_type: PolicySource,
    pub statement_index: usize,
    pub statement_id: Option<String>,
}

/// Richer result for the policy simulator. The decision is the same
/// one [`evaluate`] returns; `matched_statements` lists every
/// statement that contributed (Allow + Deny alike); and
/// `missing_context_values` lists condition keys referenced by a
/// matched statement that the request didn't supply.
#[derive(Debug, Clone)]
pub struct EvaluationDetails {
    pub decision: Decision,
    pub matched_statements: Vec<MatchedStatement>,
    pub missing_context_values: Vec<String>,
}

/// Same evaluation logic as [`evaluate`], plus matched-statement and
/// missing-context-key tracking. Used by the IAM simulator; not on
/// the hot authz path.
pub fn evaluate_detailed(
    req: &AuthzRequest,
    ctx: &EvalContext,
    attrs: &PolicyAttributions,
) -> EvaluationDetails {
    let decision = evaluate(req, ctx);

    let mut matched: Vec<MatchedStatement> = Vec::new();
    collect_matches(
        &mut matched,
        ctx.identity_policies,
        attrs.identity,
        req,
        false,
        PolicySource::Identity,
    );
    if let Some(p) = ctx.permissions_boundary {
        collect_matches_one(
            &mut matched,
            p,
            attrs.permissions_boundary,
            req,
            false,
            PolicySource::PermissionsBoundary,
        );
    }
    if let Some(p) = ctx.session_policy {
        collect_matches_one(
            &mut matched,
            p,
            attrs.session,
            req,
            false,
            PolicySource::Session,
        );
    }
    collect_matches(
        &mut matched,
        ctx.scps,
        attrs.scps,
        req,
        false,
        PolicySource::Scp,
    );
    if let Some(p) = ctx.resource_policy {
        collect_matches_one(
            &mut matched,
            p,
            attrs.resource,
            req,
            true,
            PolicySource::Resource,
        );
    }

    let missing_context_values = collect_missing_context_keys(req, ctx);

    EvaluationDetails {
        decision,
        matched_statements: matched,
        missing_context_values,
    }
}

fn collect_matches(
    out: &mut Vec<MatchedStatement>,
    policies: &[PolicyDocument],
    attributions: &[PolicyAttribution],
    req: &AuthzRequest,
    resource_policy: bool,
    fallback_source: PolicySource,
) {
    for (i, p) in policies.iter().enumerate() {
        let attr = attributions.get(i);
        for (j, s) in p.statements.iter().enumerate() {
            if stmt_matches(s, req, resource_policy) {
                out.push(MatchedStatement {
                    source_id: attr
                        .map(|a| a.source_id.clone())
                        .unwrap_or_else(|| format!("policy-{i}")),
                    source_type: attr
                        .map(|a| a.source_type.clone())
                        .unwrap_or_else(|| fallback_source.clone()),
                    statement_index: j,
                    statement_id: s.sid.clone(),
                });
            }
        }
    }
}

fn collect_matches_one(
    out: &mut Vec<MatchedStatement>,
    policy: &PolicyDocument,
    attribution: Option<&PolicyAttribution>,
    req: &AuthzRequest,
    resource_policy: bool,
    fallback_source: PolicySource,
) {
    for (j, s) in policy.statements.iter().enumerate() {
        if stmt_matches(s, req, resource_policy) {
            out.push(MatchedStatement {
                source_id: attribution
                    .map(|a| a.source_id.clone())
                    .unwrap_or_else(|| "policy".to_string()),
                source_type: attribution
                    .map(|a| a.source_type.clone())
                    .unwrap_or_else(|| fallback_source.clone()),
                statement_index: j,
                statement_id: s.sid.clone(),
            });
        }
    }
}

/// Walk the matched-statement set's condition blocks and report any
/// referenced key that the request didn't supply. Best-effort:
/// statements that didn't match wouldn't have surfaced their keys at
/// runtime anyway, so they're omitted.
fn collect_missing_context_keys(req: &AuthzRequest, ctx: &EvalContext) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut visit = |policies: &[PolicyDocument], resource_policy: bool| {
        for p in policies {
            for s in &p.statements {
                if !stmt_matches_ignoring_condition(s, req, resource_policy) {
                    continue;
                }
                let Some(cb) = &s.condition else { continue };
                for c in &cb.conditions {
                    let key = c.key.as_str();
                    let stripped = key
                        .strip_prefix("ForAllValues:")
                        .or_else(|| key.strip_prefix("ForAnyValue:"))
                        .unwrap_or(key);
                    if !req.context.contains_key(stripped) && seen.insert(stripped.to_string()) {
                        missing.push(stripped.to_string());
                    }
                }
            }
        }
    };
    visit(ctx.identity_policies, false);
    if let Some(p) = ctx.permissions_boundary {
        visit(std::slice::from_ref(p), false);
    }
    if let Some(p) = ctx.session_policy {
        visit(std::slice::from_ref(p), false);
    }
    visit(ctx.scps, false);
    if let Some(p) = ctx.resource_policy {
        visit(std::slice::from_ref(p), true);
    }
    missing
}

/// Same as `stmt_matches` but skips the condition check. Used by the
/// missing-context-keys scan so a condition key not yet supplied
/// doesn't disqualify the statement from contributing its key list.
fn stmt_matches_ignoring_condition(
    s: &Statement,
    req: &AuthzRequest,
    resource_policy: bool,
) -> bool {
    if !action_matches(s, req.action) {
        return false;
    }
    if !resource_matches_with_subst(s, req) {
        return false;
    }
    if resource_policy && !principal_matches(s, req) {
        return false;
    }
    true
}

pub fn evaluate(req: &AuthzRequest, ctx: &EvalContext) -> Decision {
    for p in ctx.identity_policies {
        if any_explicit_deny(p, req, false) {
            return Decision::ExplicitDeny;
        }
    }
    if let Some(p) = ctx.permissions_boundary
        && any_explicit_deny(p, req, false)
    {
        return Decision::ExplicitDeny;
    }
    if let Some(p) = ctx.session_policy
        && any_explicit_deny(p, req, false)
    {
        return Decision::ExplicitDeny;
    }
    for p in ctx.scps {
        if any_explicit_deny(p, req, false) {
            return Decision::ExplicitDeny;
        }
    }
    if let Some(p) = ctx.resource_policy
        && any_explicit_deny(p, req, true)
    {
        return Decision::ExplicitDeny;
    }

    if !ctx.scps.is_empty() {
        for p in ctx.scps {
            if !any_allow(p, req, false) {
                return Decision::ImplicitDeny;
            }
        }
    }

    let identity_allows = ctx
        .identity_policies
        .iter()
        .any(|p| any_allow(p, req, false));
    let resource_allows = ctx
        .resource_policy
        .map(|p| any_allow(p, req, true))
        .unwrap_or(false);

    let resource_acct = resource_account(req.resource_arn);
    let same_account = match resource_acct {
        Some(ref a) => a == req.principal_account,
        None => true,
    };

    let allowed_by_grant = if ctx.resource_policy.is_some() {
        if same_account {
            identity_allows || resource_allows
        } else {
            identity_allows && resource_allows
        }
    } else {
        identity_allows
    };

    if !allowed_by_grant {
        return Decision::ImplicitDeny;
    }

    if let Some(b) = ctx.permissions_boundary
        && !any_allow(b, req, false)
        && !(ctx.resource_policy.is_some() && resource_allows)
    {
        return Decision::ImplicitDeny;
    }

    if let Some(s) = ctx.session_policy
        && !any_allow(s, req, false)
        && !(ctx.resource_policy.is_some() && resource_allows)
    {
        return Decision::ImplicitDeny;
    }

    Decision::Allow
}

fn any_explicit_deny(policy: &PolicyDocument, req: &AuthzRequest, resource_policy: bool) -> bool {
    policy
        .statements
        .iter()
        .any(|s| s.effect == Effect::Deny && stmt_matches(s, req, resource_policy))
}

fn any_allow(policy: &PolicyDocument, req: &AuthzRequest, resource_policy: bool) -> bool {
    policy
        .statements
        .iter()
        .any(|s| s.effect == Effect::Allow && stmt_matches(s, req, resource_policy))
}

fn stmt_matches(s: &Statement, req: &AuthzRequest, resource_policy: bool) -> bool {
    if !action_matches(s, req.action) {
        return false;
    }
    if !resource_matches_with_subst(s, req) {
        return false;
    }
    if resource_policy && !principal_matches(s, req) {
        return false;
    }
    if let Some(cb) = &s.condition
        && !condition_matches_with_subst(cb, req)
    {
        return false;
    }
    true
}

fn action_matches(s: &Statement, action: &str) -> bool {
    // Actions don't typically use policy variables — but substituting
    // is cheap and correct for the rare case someone sticks a var in
    // an action string (e.g. via templating).
    if let Some(actions) = &s.action {
        return actions
            .iter()
            .any(|p| glob::matches(&p.to_lowercase(), &action.to_lowercase()));
    }
    if let Some(actions) = &s.not_action {
        return !actions
            .iter()
            .any(|p| glob::matches(&p.to_lowercase(), &action.to_lowercase()));
    }
    false
}

fn resource_matches_with_subst(s: &Statement, req: &AuthzRequest) -> bool {
    if let Some(rs) = &s.resource {
        return rs
            .iter()
            .any(|p| glob::matches_arn(&substitute(p, req), req.resource_arn));
    }
    if let Some(rs) = &s.not_resource {
        return !rs
            .iter()
            .any(|p| glob::matches_arn(&substitute(p, req), req.resource_arn));
    }
    true
}

fn principal_matches(s: &Statement, req: &AuthzRequest) -> bool {
    if let Some(p) = &s.principal {
        return principal_set_matches(p, req);
    }
    if let Some(p) = &s.not_principal {
        return !principal_set_matches(p, req);
    }
    false
}

fn principal_set_matches(p: &Principal, req: &AuthzRequest) -> bool {
    let aws_match = |list: &[String]| -> bool {
        list.iter().any(|entry| {
            entry == "*"
                || entry == req.principal_arn
                || entry == req.principal_account
                || entry == &format!("arn:aws:iam::{}:root", req.principal_account)
        })
    };
    match p {
        Principal::Wildcard => true,
        Principal::Aws(list) => aws_match(list),
        Principal::Service(_) => false,
        Principal::Federated(_) => false,
        Principal::CanonicalUser(_) => false,
        Principal::Mixed { aws, .. } => aws_match(aws),
    }
}

fn resource_account(arn: &str) -> Option<String> {
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    if parts.len() < 5 {
        return None;
    }
    let acct = parts[4];
    if acct.is_empty() {
        None
    } else {
        Some(acct.to_string())
    }
}

fn condition_matches_with_subst(block: &ConditionBlock, req: &AuthzRequest) -> bool {
    block.conditions.iter().all(|c| {
        // Substitute variables in policy-side condition values before
        // matching. Most commonly used with `${aws:PrincipalTag/<key>}`
        // and `${aws:username}` in resource-style condition values.
        let substituted: Condition = if c.values.iter().any(|v| v.contains("${")) {
            Condition {
                key: c.key.clone(),
                operator: c.operator,
                values: c.values.iter().map(|v| substitute(v, req)).collect(),
            }
        } else {
            c.clone()
        };
        condition_eval(&substituted, req.context)
    })
}

/// Substitute IAM policy variables in `template` using values
/// derived from the request. Supported:
///
///   `${aws:PrincipalArn}`     — full principal ARN
///   `${aws:PrincipalAccount}` — principal account ID
///   `${aws:username}`         — IAM user name (suffix of user ARN)
///   `${aws:userid}`           — same as `${aws:username}` for users
///   `${<context-key>}`        — any key in the request context map
///
/// Unknown variables are left as the literal `${...}` string so a
/// typo doesn't accidentally widen a policy. AWS documents this
/// "leave literal" behavior for unrecognised variables, which is
/// safer than silently dropping the variable to an empty string.
pub fn substitute(template: &str, req: &AuthzRequest) -> String {
    if !template.contains("${") {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len()
            && bytes[i] == b'$'
            && bytes[i + 1] == b'{'
            && let Some(end_rel) = template[i + 2..].find('}')
        {
            let var = &template[i + 2..i + 2 + end_rel];
            if let Some(value) = lookup_variable(var, req) {
                out.push_str(&value);
            } else {
                // Unrecognised — keep the literal.
                out.push_str(&template[i..i + 2 + end_rel + 1]);
            }
            i += 2 + end_rel + 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn lookup_variable(var: &str, req: &AuthzRequest) -> Option<String> {
    match var {
        "aws:PrincipalArn" => Some(req.principal_arn.to_string()),
        "aws:PrincipalAccount" => Some(req.principal_account.to_string()),
        "aws:username" | "aws:userid" => Some(extract_principal_name(req.principal_arn)),
        // The wildcard literal ${*}, ${$}, ${?} are supported by AWS to
        // emit `*`, `$`, `?` literally. Pass through.
        "*" => Some("*".to_string()),
        "$" => Some("$".to_string()),
        "?" => Some("?".to_string()),
        other => req.context.get(other).map(|v| match v {
            ContextValue::String(s) => s.clone(),
            ContextValue::StringList(v) => v.first().cloned().unwrap_or_default(),
            ContextValue::Number(n) => format_number(*n),
            ContextValue::Bool(b) => b.to_string(),
            ContextValue::Date(d) => d.to_rfc3339(),
            ContextValue::Ip(s) => s.clone(),
        }),
    }
}

/// Extract the trailing name from a principal ARN. For
/// `arn:aws:iam::123:user/alice` returns `alice`; for
/// `arn:aws:iam::123:role/MyRole` returns `MyRole`. Everything else
/// (federated, root, malformed) falls back to the full ARN.
fn extract_principal_name(arn: &str) -> String {
    if let Some(slash) = arn.rfind('/') {
        return arn[slash + 1..].to_string();
    }
    arn.to_string()
}

fn condition_eval(c: &Condition, context: &HashMap<String, ContextValue>) -> bool {
    let val = context.get(c.key.as_str());

    if c.operator.base == BaseOperator::Null {
        let absent =
            val.is_none() || matches!(val, Some(ContextValue::StringList(v)) if v.is_empty());
        let expect_null = c.values.iter().any(|v| v == "true");
        return absent == expect_null;
    }

    if val.is_none() {
        return c.operator.if_exists;
    }
    let val = val.unwrap();
    let context_strings = val.as_strings();

    match c.operator.set_qualifier {
        SetQualifier::None => {
            let single = context_strings.first().cloned().unwrap_or_default();
            scalar_match(c.operator.base, &single, &c.values, val)
        }
        SetQualifier::ForAllValues => {
            !context_strings.is_empty()
                && context_strings
                    .iter()
                    .all(|cv| scalar_match(c.operator.base, cv, &c.values, val))
        }
        SetQualifier::ForAnyValue => context_strings
            .iter()
            .any(|cv| scalar_match(c.operator.base, cv, &c.values, val)),
    }
}

fn scalar_match(
    op: BaseOperator,
    context_value: &str,
    policy_values: &[String],
    raw: &ContextValue,
) -> bool {
    match op {
        BaseOperator::StringEquals | BaseOperator::ArnEquals => {
            policy_values.iter().any(|p| p == context_value)
        }
        BaseOperator::StringNotEquals | BaseOperator::ArnNotEquals => {
            !policy_values.iter().any(|p| p == context_value)
        }
        BaseOperator::StringEqualsIgnoreCase => policy_values
            .iter()
            .any(|p| p.eq_ignore_ascii_case(context_value)),
        BaseOperator::StringNotEqualsIgnoreCase => !policy_values
            .iter()
            .any(|p| p.eq_ignore_ascii_case(context_value)),
        BaseOperator::StringLike => policy_values
            .iter()
            .any(|p| glob::matches(p, context_value)),
        BaseOperator::StringNotLike => !policy_values
            .iter()
            .any(|p| glob::matches(p, context_value)),
        BaseOperator::ArnLike => policy_values
            .iter()
            .any(|p| glob::matches_arn(p, context_value)),
        BaseOperator::ArnNotLike => !policy_values
            .iter()
            .any(|p| glob::matches_arn(p, context_value)),
        BaseOperator::NumericEquals => num_cmp(context_value, policy_values, |a, b| a == b),
        BaseOperator::NumericNotEquals => num_cmp(context_value, policy_values, |a, b| a != b),
        BaseOperator::NumericLessThan => num_cmp(context_value, policy_values, |a, b| a < b),
        BaseOperator::NumericLessThanEquals => num_cmp(context_value, policy_values, |a, b| a <= b),
        BaseOperator::NumericGreaterThan => num_cmp(context_value, policy_values, |a, b| a > b),
        BaseOperator::NumericGreaterThanEquals => {
            num_cmp(context_value, policy_values, |a, b| a >= b)
        }
        BaseOperator::DateEquals => date_cmp(context_value, raw, policy_values, |a, b| a == b),
        BaseOperator::DateNotEquals => date_cmp(context_value, raw, policy_values, |a, b| a != b),
        BaseOperator::DateLessThan => date_cmp(context_value, raw, policy_values, |a, b| a < b),
        BaseOperator::DateLessThanEquals => {
            date_cmp(context_value, raw, policy_values, |a, b| a <= b)
        }
        BaseOperator::DateGreaterThan => date_cmp(context_value, raw, policy_values, |a, b| a > b),
        BaseOperator::DateGreaterThanEquals => {
            date_cmp(context_value, raw, policy_values, |a, b| a >= b)
        }
        BaseOperator::Bool => policy_values.iter().any(|p| {
            let cv_bool = parse_bool(context_value);
            let pv_bool = parse_bool(p);
            match (cv_bool, pv_bool) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        }),
        BaseOperator::BinaryEquals => policy_values.iter().any(|p| p == context_value),
        BaseOperator::IpAddress => ip_match(context_value, policy_values),
        BaseOperator::NotIpAddress => !ip_match(context_value, policy_values),
        BaseOperator::Null => false,
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn num_cmp(cv: &str, pvs: &[String], cmp: impl Fn(f64, f64) -> bool) -> bool {
    let cv_n: f64 = match cv.parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    pvs.iter().any(|p| match p.parse::<f64>() {
        Ok(pn) => cmp(cv_n, pn),
        Err(_) => false,
    })
}

fn date_cmp(
    cv: &str,
    raw: &ContextValue,
    pvs: &[String],
    cmp: impl Fn(DateTime<Utc>, DateTime<Utc>) -> bool,
) -> bool {
    let cv_dt = match raw {
        ContextValue::Date(d) => *d,
        _ => match parse_date(cv) {
            Some(d) => d,
            None => return false,
        },
    };
    pvs.iter().any(|p| match parse_date(p) {
        Some(pd) => cmp(cv_dt, pd),
        None => false,
    })
}

fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(n) = s.parse::<i64>() {
        return Utc.timestamp_opt(n, 0).single();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

fn ip_match(cv: &str, pvs: &[String]) -> bool {
    let addr = match IpAddr::from_str(cv) {
        Ok(a) => a,
        Err(_) => return false,
    };
    pvs.iter().any(|p| {
        if let Ok(net) = IpNet::from_str(p) {
            return net.contains(&addr);
        }
        if let Ok(other) = IpAddr::from_str(p) {
            return other == addr;
        }
        false
    })
}
