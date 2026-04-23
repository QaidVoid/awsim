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

    let identity_allows = ctx.identity_policies.iter().any(|p| any_allow(p, req, false));
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
    if !resource_matches(s, req.resource_arn) {
        return false;
    }
    if resource_policy && !principal_matches(s, req) {
        return false;
    }
    if let Some(cb) = &s.condition
        && !condition_matches(cb, req.context)
    {
        return false;
    }
    true
}

fn action_matches(s: &Statement, action: &str) -> bool {
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

fn resource_matches(s: &Statement, resource: &str) -> bool {
    if let Some(rs) = &s.resource {
        return rs.iter().any(|p| glob::matches_arn(p, resource));
    }
    if let Some(rs) = &s.not_resource {
        return !rs.iter().any(|p| glob::matches_arn(p, resource));
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

fn condition_matches(block: &ConditionBlock, context: &HashMap<String, ContextValue>) -> bool {
    block.conditions.iter().all(|c| condition_eval(c, context))
}

fn condition_eval(c: &Condition, context: &HashMap<String, ContextValue>) -> bool {
    let val = context.get(c.key.as_str());

    if c.operator.base == BaseOperator::Null {
        let absent = val.is_none()
            || matches!(val, Some(ContextValue::StringList(v)) if v.is_empty());
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
        BaseOperator::NumericLessThanEquals => {
            num_cmp(context_value, policy_values, |a, b| a <= b)
        }
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
