use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};
use uuid::Uuid;

use crate::operations::buses::ensure_default_bus;
use crate::state::{EventBridgeState, Rule, StoredEvent};

// ---------------------------------------------------------------------------
// PutEvents
// ---------------------------------------------------------------------------

pub fn put_events(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let entries = input["Entries"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Entries is required"))?;

    ensure_default_bus(state, ctx);

    let mut result_entries: Vec<Value> = Vec::new();
    let mut failed_count = 0u64;

    for entry in entries {
        let bus_name = entry["EventBusName"].as_str().unwrap_or("default");
        let source = entry["Source"].as_str().unwrap_or("").to_string();
        let detail_type = entry["DetailType"].as_str().unwrap_or("").to_string();
        let detail = entry["Detail"].as_str().unwrap_or("{}").to_string();

        // Validate required fields
        if source.is_empty() {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "InvalidParameterValue",
                "ErrorMessage": "Source is required",
            }));
            continue;
        }

        if detail_type.is_empty() {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "InvalidParameterValue",
                "ErrorMessage": "DetailType is required",
            }));
            continue;
        }

        // Ensure bus exists
        let bus_policy = match state.event_buses.get(bus_name) {
            Some(b) => b.policy.clone(),
            None => {
                failed_count += 1;
                result_entries.push(json!({
                    "ErrorCode": "ResourceNotFoundException",
                    "ErrorMessage": format!("Event bus {bus_name} does not exist"),
                }));
                continue;
            }
        };

        // Resource policy gate for cross-account writes. AWS evaluates
        // the bus's resource policy against the caller's account; if
        // there's no `events:PutEvents` allow, the entry fails with
        // AccessDeniedException.
        if !is_authorized_caller(
            &ctx.account_id,
            &bus_policy,
            &source_account_from_entry(entry, ctx),
        ) {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "AccessDeniedException",
                "ErrorMessage": format!("Cross-account PutEvents on `{bus_name}` requires a matching resource policy."),
            }));
            continue;
        }

        let resources: Vec<String> = entry["Resources"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let event_id = Uuid::new_v4().to_string();

        // Build the canonical event object that rule patterns match
        // against. `detail` is parsed back into an object so nested
        // patterns like `{"detail": {"status": ["FAILED"]}}` work.
        let parsed_detail: Value =
            serde_json::from_str(&detail).unwrap_or_else(|_| Value::Object(Default::default()));
        let original_event = json!({
            "id": event_id,
            "source": source,
            "detail-type": detail_type,
            "detail": parsed_detail,
            "resources": resources,
            "account": ctx.account_id,
            "region": ctx.region,
        });

        // Match event against rules on the bus
        let matched_rules =
            match_event_against_rules_with_targets(state, bus_name, &original_event, ctx);

        if !matched_rules.is_empty() {
            info!(
                event_id = %event_id,
                source = %source,
                detail_type = %detail_type,
                bus = %bus_name,
                matched_rules = ?matched_rules,
                "Event matched rules"
            );
        } else {
            info!(
                event_id = %event_id,
                source = %source,
                detail_type = %detail_type,
                bus = %bus_name,
                "Event delivered (no rules matched)"
            );
        }

        let stored = StoredEvent {
            event_id: event_id.clone(),
            source,
            detail_type,
            detail,
            event_bus_name: bus_name.to_string(),
            resources,
            matched_rules,
        };
        state.recent_events.insert(event_id.clone(), stored);

        result_entries.push(json!({ "EventId": event_id }));
    }

    Ok(json!({
        "FailedEntryCount": failed_count,
        "Entries": result_entries,
    }))
}

/// TestEventPattern - evaluate an EventPattern against an Event using
/// the exact same matcher PutEvents routes with, so the UI's "which
/// rules would match" preview can never disagree with real delivery.
pub fn test_event_pattern(input: &Value) -> Result<Value, AwsError> {
    let pattern_str = input
        .get("EventPattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameterValue", "EventPattern is required")
        })?;
    let event_str = input
        .get("Event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Event is required"))?;
    let pattern: Value = serde_json::from_str(pattern_str).map_err(|e| {
        AwsError::bad_request(
            "InvalidEventPatternException",
            format!("Event pattern is not valid JSON: {e}"),
        )
    })?;
    let event: Value = serde_json::from_str(event_str).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterValue",
            format!("Event is not valid JSON: {e}"),
        )
    })?;
    Ok(json!({ "Result": pattern_matches(&pattern, &event) }))
}

// ---------------------------------------------------------------------------
// Pattern matching helpers
// ---------------------------------------------------------------------------

/// Return the names of all ENABLED rules on `bus_name` that match this event.
/// For each matched rule, emit an `eventbridge:TargetInvocation` InternalEvent
/// for every configured target so the integration layer can dispatch them.
fn match_event_against_rules_with_targets(
    state: &EventBridgeState,
    bus_name: &str,
    original_event: &Value,
    ctx: &RequestContext,
) -> Vec<String> {
    let bus = match state.event_buses.get(bus_name) {
        Some(b) => b,
        None => return vec![],
    };

    let mut matched_rule_names: Vec<String> = Vec::new();

    for rule in bus.rules.values() {
        if rule.state != "ENABLED" {
            continue;
        }
        if !matches_pattern(rule, original_event) {
            continue;
        }

        matched_rule_names.push(rule.name.clone());

        // Emit one InternalEvent per target so the router can dispatch them.
        if let Some(ref event_bus) = ctx.event_bus {
            for target in &rule.targets {
                event_bus.publish(InternalEvent {
                    source: "events".to_string(),
                    event_type: "eventbridge:TargetInvocation".to_string(),
                    region: ctx.region.clone(),
                    account_id: ctx.account_id.clone(),
                    detail: json!({
                        "targetArn": target.arn,
                        "targetId": target.id,
                        "ruleName": rule.name,
                        "event": original_event,
                    }),
                });
            }
        }
    }

    matched_rule_names
}

/// Check whether an event JSON matches the rule's EventPattern. A rule
/// with no EventPattern (schedule-only rule) never matches PutEvents.
///
/// Supports the full AWS EventBridge pattern syntax: arbitrary nested
/// fields (recurses into `detail`), and operator objects of the form
/// `{prefix|suffix|exists|numeric|anything-but|cidr|equals-ignore-case}`.
fn matches_pattern(rule: &Rule, event: &Value) -> bool {
    let pattern_str = match &rule.event_pattern {
        Some(p) => p,
        None => return false,
    };
    let pattern: Value = match serde_json::from_str(pattern_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(rule = %rule.name, error = %e, "Failed to parse event pattern");
            return false;
        }
    };
    pattern_matches(&pattern, event)
}

/// Recursive matcher: each top-level pattern key either descends into a
/// nested object on the event or applies an array of leaf conditions.
fn pattern_matches(pattern: &Value, event: &Value) -> bool {
    let Some(obj) = pattern.as_object() else {
        return false;
    };
    obj.iter().all(|(key, conditions)| {
        let event_value = event.get(key);
        // Nested pattern: pattern is `{"detail": {"status": [...]}}` and
        // we want to recurse with the event's `detail` subtree.
        if conditions.is_object() && !is_operator_object(conditions) {
            match event_value {
                Some(nested) => pattern_matches(conditions, nested),
                None => false,
            }
        } else if let Some(arr) = conditions.as_array() {
            arr.iter().any(|c| matches_single_condition(c, event_value))
        } else {
            false
        }
    })
}

/// Detect an operator-object leaf so the recursion above doesn't treat
/// `{"prefix": "x"}` as a nested field block.
fn is_operator_object(v: &Value) -> bool {
    let Some(obj) = v.as_object() else {
        return false;
    };
    obj.keys().any(|k| {
        matches!(
            k.as_str(),
            "prefix"
                | "suffix"
                | "exists"
                | "numeric"
                | "anything-but"
                | "equals-ignore-case"
                | "cidr"
                | "wildcard"
        )
    })
}

fn event_str(v: Option<&Value>) -> Option<String> {
    let v = v?;
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn event_num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| match x {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn matches_single_condition(condition: &Value, attr: Option<&Value>) -> bool {
    match condition {
        // Literal string equality (AWS doesn't coerce types).
        Value::String(s) => event_str(attr).map(|v| v == *s).unwrap_or(false),
        // Numeric equality.
        Value::Number(n) => event_num(attr)
            .map(|v| Some(v) == n.as_f64())
            .unwrap_or(false),
        Value::Bool(b) => attr
            .and_then(|v| v.as_bool())
            .map(|v| v == *b)
            .unwrap_or(false),
        Value::Null => attr.is_none() || matches!(attr, Some(Value::Null)),
        Value::Object(obj) => {
            if let Some(prefix) = obj.get("prefix").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .map(|v| v.starts_with(prefix))
                    .unwrap_or(false);
            }
            if let Some(suffix) = obj.get("suffix").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .map(|v| v.ends_with(suffix))
                    .unwrap_or(false);
            }
            if let Some(target) = obj.get("equals-ignore-case").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .map(|v| v.eq_ignore_ascii_case(target))
                    .unwrap_or(false);
            }
            if let Some(exists) = obj.get("exists").and_then(|v| v.as_bool()) {
                return attr.is_some() == exists;
            }
            if let Some(arr) = obj.get("numeric").and_then(|v| v.as_array()) {
                let Some(value) = event_num(attr) else {
                    return false;
                };
                let mut i = 0;
                while i + 1 < arr.len() {
                    let op = arr[i].as_str().unwrap_or("");
                    let target = arr[i + 1].as_f64().unwrap_or(0.0);
                    let ok = match op {
                        "=" => (value - target).abs() < f64::EPSILON,
                        "<" => value < target,
                        "<=" => value <= target,
                        ">" => value > target,
                        ">=" => value >= target,
                        _ => false,
                    };
                    if !ok {
                        return false;
                    }
                    i += 2;
                }
                return true;
            }
            if let Some(ab) = obj.get("anything-but") {
                return matches_anything_but(ab, attr);
            }
            if let Some(cidr) = obj.get("cidr").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .as_deref()
                    .map(|v| ip_in_cidr(v, cidr))
                    .unwrap_or(false);
            }
            if let Some(pat) = obj.get("wildcard").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .as_deref()
                    .map(|v| wildcard_match(pat, v))
                    .unwrap_or(false);
            }
            false
        }
        Value::Array(_) => false,
    }
}

/// Implementation of the `anything-but` operator. AWS accepts a single
/// string, a list of strings, or an inner operator object that uses
/// `prefix` / `suffix` / `equals-ignore-case`.
fn matches_anything_but(spec: &Value, attr: Option<&Value>) -> bool {
    match spec {
        Value::String(s) => event_str(attr).map(|v| v != *s).unwrap_or(false),
        Value::Array(arr) => {
            let Some(actual) = event_str(attr) else {
                return false;
            };
            !arr.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == actual.as_str())
        }
        Value::Object(obj) => {
            // anything-but reuses prefix/suffix/equals-ignore-case as the
            // negative side: match iff the inner condition does NOT.
            if let Some(p) = obj.get("prefix").and_then(|v| v.as_str()) {
                return event_str(attr).map(|v| !v.starts_with(p)).unwrap_or(false);
            }
            if let Some(s) = obj.get("suffix").and_then(|v| v.as_str()) {
                return event_str(attr).map(|v| !v.ends_with(s)).unwrap_or(false);
            }
            if let Some(t) = obj.get("equals-ignore-case").and_then(|v| v.as_str()) {
                return event_str(attr)
                    .map(|v| !v.eq_ignore_ascii_case(t))
                    .unwrap_or(false);
            }
            false
        }
        _ => false,
    }
}

/// Minimal CIDR match supporting IPv4 and IPv6. Returns false on
/// malformed input rather than panicking.
fn ip_in_cidr(addr: &str, cidr: &str) -> bool {
    let (network, prefix_str) = match cidr.split_once('/') {
        Some(p) => p,
        None => return false,
    };
    let prefix: u32 = match prefix_str.parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if let (Ok(a), Ok(b)) = (
        addr.parse::<std::net::Ipv4Addr>(),
        network.parse::<std::net::Ipv4Addr>(),
    ) {
        if prefix > 32 {
            return false;
        }
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        return (u32::from(a) & mask) == (u32::from(b) & mask);
    }
    if let (Ok(a), Ok(b)) = (
        addr.parse::<std::net::Ipv6Addr>(),
        network.parse::<std::net::Ipv6Addr>(),
    ) {
        if prefix > 128 {
            return false;
        }
        let mask = if prefix == 0 {
            0u128
        } else {
            u128::MAX << (128 - prefix)
        };
        return (u128::from(a) & mask) == (u128::from(b) & mask);
    }
    false
}

/// Greedy `*`-only wildcard matcher (AWS supports `*` in EventBridge
/// patterns; `?` is not part of the pattern syntax). Backtracks so
/// patterns like `*foo*bar*` work on long strings.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut pi = 0;
    let mut ti = 0;
    let mut star_p: Option<usize> = None;
    let mut star_t: usize = 0;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Resolve the account that is publishing this entry. AWS uses
/// `Source` only as a free-form string; the account for the policy
/// check is the caller's request context, not the entry. We honour an
/// explicit `Account` field on the entry for tests that want to
/// simulate a foreign caller without spinning up a separate context.
fn source_account_from_entry(entry: &Value, ctx: &RequestContext) -> String {
    entry["Account"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| ctx.account_id.clone())
}

/// Decide whether the caller is authorized to PutEvents on this bus.
/// Same-account writes always pass; cross-account writes need at
/// least one resource-policy statement with Effect=Allow,
/// Action=`events:PutEvents` (or `events:*`/`*`), and a Principal
/// that matches the caller (root ARN, exact account id, or `*`).
fn is_authorized_caller(
    bus_owner: &str,
    bus_policy: &Option<String>,
    caller_account: &str,
) -> bool {
    if caller_account == bus_owner {
        return true;
    }
    let Some(raw) = bus_policy else {
        return false;
    };
    let Ok(doc): Result<Value, _> = serde_json::from_str(raw) else {
        return false;
    };
    let statements = match doc.get("Statement") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(s @ Value::Object(_)) => vec![s.clone()],
        _ => return false,
    };
    for stmt in statements {
        if stmt.get("Effect").and_then(Value::as_str) != Some("Allow") {
            continue;
        }
        if !statement_action_matches(&stmt) {
            continue;
        }
        if statement_principal_matches(&stmt, caller_account) {
            return true;
        }
    }
    false
}

fn statement_action_matches(stmt: &Value) -> bool {
    let actions: Vec<String> = match stmt.get("Action") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => return false,
    };
    actions
        .iter()
        .any(|a| matches!(a.as_str(), "events:PutEvents" | "events:*" | "*"))
}

fn statement_principal_matches(stmt: &Value, caller_account: &str) -> bool {
    let principal = stmt.get("Principal");
    match principal {
        Some(Value::String(s)) if s == "*" => true,
        Some(Value::Object(map)) => {
            let aws = map.get("AWS");
            match aws {
                Some(Value::String(arn)) => arn_or_account_matches(arn, caller_account),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .any(|arn| arn_or_account_matches(arn, caller_account)),
                _ => false,
            }
        }
        _ => false,
    }
}

fn arn_or_account_matches(arn: &str, caller_account: &str) -> bool {
    if arn == "*" {
        return true;
    }
    if arn == caller_account {
        return true;
    }
    if let Some(rest) = arn.strip_prefix("arn:aws:iam::")
        && let Some(account) = rest.split(':').next()
    {
        return account == caller_account;
    }
    false
}

#[cfg(test)]
mod pattern_tests {
    use super::*;
    use crate::state::Rule;

    fn rule_with(pattern: &str) -> Rule {
        Rule {
            name: "r".into(),
            arn: "arn:aws:events:us-east-1:000000000000:rule/r".into(),
            event_pattern: Some(pattern.into()),
            schedule_expression: None,
            description: String::new(),
            state: "ENABLED".into(),
            event_bus_name: "default".into(),
            targets: vec![],
            managed_by: None,
        }
    }

    #[test]
    fn nested_detail_field_matches() {
        let rule = rule_with(r#"{"detail":{"status":["FAILED"]}}"#);
        let event = json!({
            "source": "myapp",
            "detail-type": "Job",
            "detail": { "status": "FAILED" },
        });
        assert!(matches_pattern(&rule, &event));
    }

    #[test]
    fn prefix_operator_matches() {
        let rule = rule_with(r#"{"source":[{"prefix":"aws."}]}"#);
        let yes = json!({ "source": "aws.s3" });
        let no = json!({ "source": "myapp" });
        assert!(matches_pattern(&rule, &yes));
        assert!(!matches_pattern(&rule, &no));
    }

    #[test]
    fn anything_but_array_excludes_listed_values() {
        let rule = rule_with(r#"{"detail-type":[{"anything-but":["X","Y"]}]}"#);
        assert!(matches_pattern(&rule, &json!({ "detail-type": "Z" })));
        assert!(!matches_pattern(&rule, &json!({ "detail-type": "X" })));
    }

    #[test]
    fn numeric_operator_matches_range() {
        let rule = rule_with(r#"{"detail":{"price":[{"numeric":[">=",10,"<",20]}]}}"#);
        assert!(matches_pattern(
            &rule,
            &json!({ "detail": { "price": 15 } })
        ));
        assert!(!matches_pattern(
            &rule,
            &json!({ "detail": { "price": 25 } })
        ));
    }

    #[test]
    fn exists_operator_distinguishes_present_from_absent() {
        let rule_yes = rule_with(r#"{"detail":{"foo":[{"exists":true}]}}"#);
        let rule_no = rule_with(r#"{"detail":{"foo":[{"exists":false}]}}"#);
        assert!(matches_pattern(
            &rule_yes,
            &json!({ "detail": { "foo": 1 } })
        ));
        assert!(!matches_pattern(&rule_yes, &json!({ "detail": {} })));
        assert!(matches_pattern(&rule_no, &json!({ "detail": {} })));
    }

    #[test]
    fn wildcard_matches_glob_segment() {
        let rule = rule_with(r#"{"source":[{"wildcard":"aws.*"}]}"#);
        assert!(matches_pattern(&rule, &json!({ "source": "aws.s3" })));
        assert!(!matches_pattern(&rule, &json!({ "source": "myapp.s3" })));
    }

    #[test]
    fn cidr_matches_ipv4_in_block() {
        let rule = rule_with(r#"{"detail":{"ip":[{"cidr":"10.0.0.0/8"}]}}"#);
        assert!(matches_pattern(
            &rule,
            &json!({ "detail": { "ip": "10.1.2.3" } })
        ));
        assert!(!matches_pattern(
            &rule,
            &json!({ "detail": { "ip": "192.168.1.1" } })
        ));
    }

    #[test]
    fn unmatched_top_level_field_fails_quickly() {
        let rule = rule_with(r#"{"source":["myapp"]}"#);
        assert!(!matches_pattern(&rule, &json!({ "source": "other" })));
    }

    #[test]
    fn same_account_caller_is_always_authorized() {
        assert!(is_authorized_caller("111111111111", &None, "111111111111"));
    }

    #[test]
    fn cross_account_without_policy_is_denied() {
        assert!(!is_authorized_caller("111111111111", &None, "222222222222"));
    }

    #[test]
    fn cross_account_allow_statement_grants_access() {
        let policy = json!({
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "arn:aws:iam::222222222222:root" },
                "Action": "events:PutEvents"
            }]
        })
        .to_string();
        assert!(is_authorized_caller(
            "111111111111",
            &Some(policy),
            "222222222222"
        ));
    }

    #[test]
    fn wildcard_principal_grants_any_account() {
        let policy = json!({
            "Statement": [{ "Effect": "Allow", "Principal": "*", "Action": "events:*" }]
        })
        .to_string();
        assert!(is_authorized_caller(
            "111111111111",
            &Some(policy),
            "333333333333"
        ));
    }

    #[test]
    fn statement_for_other_account_is_ignored() {
        let policy = json!({
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "arn:aws:iam::555555555555:root" },
                "Action": "events:PutEvents"
            }]
        })
        .to_string();
        assert!(!is_authorized_caller(
            "111111111111",
            &Some(policy),
            "222222222222"
        ));
    }

    #[test]
    fn statement_without_putevents_action_is_ignored() {
        let policy = json!({
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "arn:aws:iam::222222222222:root" },
                "Action": "events:DescribeRule"
            }]
        })
        .to_string();
        assert!(!is_authorized_caller(
            "111111111111",
            &Some(policy),
            "222222222222"
        ));
    }
}
