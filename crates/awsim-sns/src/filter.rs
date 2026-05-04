use serde_json::Value;
use std::collections::HashMap;

/// Evaluate a filter policy against message attributes.
/// Returns true if the message should be delivered.
pub fn matches_filter(filter_policy: &Value, message_attributes: &HashMap<String, Value>) -> bool {
    let policy = match filter_policy.as_object() {
        Some(p) => p,
        None => return true, // Invalid policy = pass through
    };

    // ALL filter keys must match (AND logic)
    for (key, conditions) in policy {
        let attr_value = message_attributes.get(key);
        if !matches_conditions(conditions, attr_value) {
            return false;
        }
    }
    true
}

/// Evaluate a filter policy against the parsed message body. Used when
/// the subscription has `FilterPolicyScope=MessageBody`. Policy keys
/// nest to mirror the body's JSON structure: a policy of
/// `{"detail":{"status":["FAILED"]}}` matches a body whose `detail.status`
/// field equals `"FAILED"`.
pub fn matches_filter_body(filter_policy: &Value, body: &Value) -> bool {
    let policy = match filter_policy.as_object() {
        Some(p) => p,
        None => return true,
    };

    for (key, conditions) in policy {
        let body_value = body.get(key);
        // Nested object policy → recurse into the same field of the body.
        if conditions.is_object() && !is_operator_object(conditions) {
            let Some(nested_body) = body_value else {
                return false;
            };
            if !matches_filter_body(conditions, nested_body) {
                return false;
            }
            continue;
        }
        // Leaf array of conditions → reuse the same single-value matcher.
        if !matches_conditions(conditions, body_value) {
            return false;
        }
    }
    true
}

/// Detect a leaf operator object like `{"prefix": "x"}` so the body
/// matcher knows not to recurse into it (the operator object lives at
/// the same nesting depth as a value would, but is structurally a
/// directive rather than a field).
fn is_operator_object(v: &Value) -> bool {
    match v.as_object() {
        Some(obj) => obj.keys().any(|k| {
            matches!(
                k.as_str(),
                "prefix"
                    | "suffix"
                    | "exists"
                    | "numeric"
                    | "anything-but"
                    | "equals-ignore-case"
                    | "cidr"
            )
        }),
        None => false,
    }
}

fn matches_conditions(conditions: &Value, attr: Option<&Value>) -> bool {
    let conditions = match conditions.as_array() {
        Some(c) => c,
        None => return true,
    };

    // ANY condition must match (OR logic)
    for condition in conditions {
        if matches_single_condition(condition, attr) {
            return true;
        }
    }
    false
}

fn attr_str(attr: Option<&Value>) -> Option<&str> {
    attr.and_then(|a| a["Value"].as_str().or_else(|| a.as_str()))
}

/// Extract a stringified scalar from either form a filter value can take:
///
/// - `{"Value": "..."}` (SNS attribute envelope; only String reaches here)
/// - a raw JSON scalar (string/number/boolean) — used when matching
///   against a parsed message body via `FilterPolicyScope=MessageBody`.
///
/// Returns `None` for objects, arrays, and null.
fn scalar_as_string(attr: Option<&Value>) -> Option<String> {
    let v = attr?;
    if let Some(s) = v["Value"].as_str() {
        return Some(s.to_string());
    }
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn matches_single_condition(condition: &Value, attr: Option<&Value>) -> bool {
    match condition {
        // String exact match — AWS treats string and numeric values as
        // distinct types, so a string condition only matches a string
        // attribute / body value (no implicit coercion).
        Value::String(s) => attr_str(attr).map(|v| v == s).unwrap_or(false),
        // Numeric match — accept either a wrapped attribute string or a
        // raw JSON number from a parsed message body.
        Value::Number(n) => scalar_as_string(attr)
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| Some(v) == n.as_f64())
            .unwrap_or(false),
        // Object conditions: { "prefix": "..." }, { "suffix": "..." },
        // { "exists": true|false }, { "numeric": [...] },
        // { "anything-but": ... }, { "equals-ignore-case": "..." },
        // { "cidr": "..." }
        Value::Object(obj) => {
            if let Some(prefix) = obj.get("prefix").and_then(|v| v.as_str()) {
                return attr_str(attr)
                    .map(|v| v.starts_with(prefix))
                    .unwrap_or(false);
            }
            if let Some(suffix) = obj.get("suffix").and_then(|v| v.as_str()) {
                return attr_str(attr).map(|v| v.ends_with(suffix)).unwrap_or(false);
            }
            if let Some(target) = obj.get("equals-ignore-case").and_then(|v| v.as_str()) {
                return attr_str(attr)
                    .map(|v| v.eq_ignore_ascii_case(target))
                    .unwrap_or(false);
            }
            if let Some(exists) = obj.get("exists").and_then(|v| v.as_bool()) {
                return attr.is_some() == exists;
            }
            if let Some(numeric) = obj.get("numeric").and_then(|v| v.as_array()) {
                return matches_numeric(numeric, attr);
            }
            if let Some(any) = obj.get("anything-but") {
                return matches_anything_but(any, attr);
            }
            if let Some(cidr) = obj.get("cidr").and_then(|v| v.as_str()) {
                return attr_str(attr).map(|v| ip_in_cidr(v, cidr)).unwrap_or(false);
            }
            false
        }
        _ => false,
    }
}

/// Evaluate an `anything-but` clause:
/// - `{ "anything-but": "x" }` — matches if attr is present and != "x"
/// - `{ "anything-but": ["x", "y"] }` — matches if attr is present and not in list
/// - `{ "anything-but": { "prefix": "x" } }` — matches if attr is present and
///   does not start with "x"
fn matches_anything_but(spec: &Value, attr: Option<&Value>) -> bool {
    let Some(value) = attr_str(attr) else {
        return false;
    };
    match spec {
        Value::String(s) => value != s,
        Value::Array(arr) => !arr.iter().any(|item| match item {
            Value::String(s) => s == value,
            _ => false,
        }),
        Value::Object(obj) => {
            if let Some(p) = obj.get("prefix").and_then(|v| v.as_str()) {
                !value.starts_with(p)
            } else if let Some(p) = obj.get("suffix").and_then(|v| v.as_str()) {
                !value.ends_with(p)
            } else if let Some(t) = obj.get("equals-ignore-case").and_then(|v| v.as_str()) {
                !value.eq_ignore_ascii_case(t)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns true when `addr` parses as an IP address (v4 or v6) and lies
/// within the supplied `cidr` block. Mixing address families (v4 inside
/// a v6 CIDR or vice versa) returns false.
fn ip_in_cidr(addr: &str, cidr: &str) -> bool {
    use std::net::IpAddr;
    let (block, prefix_str) = match cidr.split_once('/') {
        Some(parts) => parts,
        None => return false,
    };
    let prefix: u8 = match prefix_str.parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let block_addr: IpAddr = match block.parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let value_addr: IpAddr = match addr.parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    match (block_addr, value_addr) {
        (IpAddr::V4(b), IpAddr::V4(v)) => {
            if prefix > 32 {
                return false;
            }
            let bits: u32 = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(b) & bits) == (u32::from(v) & bits)
        }
        (IpAddr::V6(b), IpAddr::V6(v)) => {
            if prefix > 128 {
                return false;
            }
            let b = u128::from(b);
            let v = u128::from(v);
            let bits: u128 = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (b & bits) == (v & bits)
        }
        _ => false,
    }
}

fn matches_numeric(conditions: &[Value], attr: Option<&Value>) -> bool {
    let val = match scalar_as_string(attr).and_then(|v| v.parse::<f64>().ok()) {
        Some(v) => v,
        None => return false,
    };

    let mut i = 0;
    while i < conditions.len() {
        let op = match conditions[i].as_str() {
            Some(o) => o,
            None => return false,
        };
        i += 1;
        let cmp_val = match conditions.get(i).and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => return false,
        };
        i += 1;

        let ok = match op {
            "=" => val == cmp_val,
            ">" => val > cmp_val,
            ">=" => val >= cmp_val,
            "<" => val < cmp_val,
            "<=" => val <= cmp_val,
            _ => return false,
        };
        if !ok {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn attr(value: &str) -> Value {
        json!({ "Value": value })
    }

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), attr(v)))
            .collect()
    }

    // --- matches_filter ---

    #[test]
    fn test_no_filter_passes_all() {
        // An empty policy object passes everything
        let policy = json!({});
        assert!(matches_filter(&policy, &attrs(&[("foo", "bar")])));
        assert!(matches_filter(&policy, &HashMap::new()));
    }

    #[test]
    fn test_invalid_policy_passes_through() {
        // Non-object policy is treated as pass-through
        let policy = json!(null);
        assert!(matches_filter(&policy, &HashMap::new()));
        let policy2 = json!("string");
        assert!(matches_filter(&policy2, &HashMap::new()));
    }

    #[test]
    fn test_string_exact_match() {
        let policy = json!({ "store": ["example_corp"] });
        assert!(matches_filter(
            &policy,
            &attrs(&[("store", "example_corp")])
        ));
        assert!(!matches_filter(&policy, &attrs(&[("store", "other_corp")])));
    }

    #[test]
    fn test_string_missing_attr_fails() {
        let policy = json!({ "store": ["example_corp"] });
        assert!(!matches_filter(&policy, &HashMap::new()));
    }

    #[test]
    fn test_string_or_logic() {
        let policy = json!({ "event": ["order-created", "order-updated"] });
        assert!(matches_filter(
            &policy,
            &attrs(&[("event", "order-created")])
        ));
        assert!(matches_filter(
            &policy,
            &attrs(&[("event", "order-updated")])
        ));
        assert!(!matches_filter(
            &policy,
            &attrs(&[("event", "order-deleted")])
        ));
    }

    #[test]
    fn test_and_logic_all_must_match() {
        let policy = json!({
            "store": ["example_corp"],
            "event": ["order-created"]
        });
        // Both match
        assert!(matches_filter(
            &policy,
            &attrs(&[("store", "example_corp"), ("event", "order-created")])
        ));
        // Only one matches
        assert!(!matches_filter(
            &policy,
            &attrs(&[("store", "example_corp"), ("event", "order-deleted")])
        ));
        // Neither matches
        assert!(!matches_filter(
            &policy,
            &attrs(&[("store", "other_corp"), ("event", "order-deleted")])
        ));
    }

    #[test]
    fn test_prefix_match() {
        let policy = json!({ "event": [{ "prefix": "order-" }] });
        assert!(matches_filter(
            &policy,
            &attrs(&[("event", "order-created")])
        ));
        assert!(matches_filter(
            &policy,
            &attrs(&[("event", "order-shipped")])
        ));
        assert!(!matches_filter(
            &policy,
            &attrs(&[("event", "invoice-created")])
        ));
    }

    #[test]
    fn test_exists_true() {
        let policy = json!({ "customer_id": [{ "exists": true }] });
        assert!(matches_filter(
            &policy,
            &attrs(&[("customer_id", "cust-1")])
        ));
        assert!(!matches_filter(&policy, &HashMap::new()));
    }

    #[test]
    fn test_exists_false() {
        let policy = json!({ "customer_id": [{ "exists": false }] });
        assert!(matches_filter(&policy, &HashMap::new()));
        assert!(!matches_filter(
            &policy,
            &attrs(&[("customer_id", "cust-1")])
        ));
    }

    #[test]
    fn test_numeric_gte() {
        let policy = json!({ "price_usd": [{ "numeric": [">=", 100] }] });
        assert!(matches_filter(&policy, &attrs(&[("price_usd", "100")])));
        assert!(matches_filter(&policy, &attrs(&[("price_usd", "250")])));
        assert!(!matches_filter(&policy, &attrs(&[("price_usd", "99")])));
        assert!(!matches_filter(&policy, &attrs(&[("price_usd", "0")])));
    }

    #[test]
    fn test_numeric_range() {
        let policy = json!({ "price_usd": [{ "numeric": [">", 10, "<=", 100] }] });
        assert!(matches_filter(&policy, &attrs(&[("price_usd", "50")])));
        assert!(matches_filter(&policy, &attrs(&[("price_usd", "100")])));
        assert!(!matches_filter(&policy, &attrs(&[("price_usd", "10")])));
        assert!(!matches_filter(&policy, &attrs(&[("price_usd", "101")])));
    }

    #[test]
    fn test_numeric_equal() {
        let policy = json!({ "qty": [{ "numeric": ["=", 5] }] });
        assert!(matches_filter(&policy, &attrs(&[("qty", "5")])));
        assert!(!matches_filter(&policy, &attrs(&[("qty", "6")])));
    }

    #[test]
    fn test_numeric_less_than() {
        let policy = json!({ "qty": [{ "numeric": ["<", 10] }] });
        assert!(matches_filter(&policy, &attrs(&[("qty", "9")])));
        assert!(!matches_filter(&policy, &attrs(&[("qty", "10")])));
    }

    #[test]
    fn test_numeric_non_numeric_attr_fails() {
        let policy = json!({ "price": [{ "numeric": [">=", 10] }] });
        assert!(!matches_filter(
            &policy,
            &attrs(&[("price", "not-a-number")])
        ));
    }

    #[test]
    fn test_suffix_match() {
        let policy = json!({ "file": [{ "suffix": ".jpg" }] });
        assert!(matches_filter(&policy, &attrs(&[("file", "photo.jpg")])));
        assert!(matches_filter(&policy, &attrs(&[("file", "x.JPG.jpg")])));
        assert!(!matches_filter(&policy, &attrs(&[("file", "doc.pdf")])));
        assert!(!matches_filter(&policy, &HashMap::new()));
    }

    #[test]
    fn test_equals_ignore_case() {
        let policy = json!({ "level": [{ "equals-ignore-case": "WARNING" }] });
        assert!(matches_filter(&policy, &attrs(&[("level", "Warning")])));
        assert!(matches_filter(&policy, &attrs(&[("level", "warning")])));
        assert!(matches_filter(&policy, &attrs(&[("level", "WARNING")])));
        assert!(!matches_filter(&policy, &attrs(&[("level", "info")])));
    }

    #[test]
    fn test_anything_but_string() {
        let policy = json!({ "kind": [{ "anything-but": "test" }] });
        assert!(matches_filter(&policy, &attrs(&[("kind", "prod")])));
        assert!(!matches_filter(&policy, &attrs(&[("kind", "test")])));
        // Missing attr does not match anything-but per AWS semantics —
        // anything-but requires the attribute to be present.
        assert!(!matches_filter(&policy, &HashMap::new()));
    }

    #[test]
    fn test_anything_but_array() {
        let policy = json!({ "kind": [{ "anything-but": ["test", "stage"] }] });
        assert!(matches_filter(&policy, &attrs(&[("kind", "prod")])));
        assert!(!matches_filter(&policy, &attrs(&[("kind", "test")])));
        assert!(!matches_filter(&policy, &attrs(&[("kind", "stage")])));
    }

    #[test]
    fn test_anything_but_prefix() {
        let policy = json!({ "kind": [{ "anything-but": { "prefix": "test-" } }] });
        assert!(matches_filter(&policy, &attrs(&[("kind", "prod-1")])));
        assert!(!matches_filter(&policy, &attrs(&[("kind", "test-1")])));
    }

    #[test]
    fn test_cidr_ipv4_match() {
        let policy = json!({ "src": [{ "cidr": "10.0.0.0/24" }] });
        assert!(matches_filter(&policy, &attrs(&[("src", "10.0.0.5")])));
        assert!(matches_filter(&policy, &attrs(&[("src", "10.0.0.255")])));
        assert!(!matches_filter(&policy, &attrs(&[("src", "10.0.1.0")])));
        assert!(!matches_filter(&policy, &attrs(&[("src", "192.168.0.1")])));
        assert!(!matches_filter(&policy, &attrs(&[("src", "not-an-ip")])));
    }

    #[test]
    fn test_cidr_ipv4_zero_prefix_matches_all() {
        let policy = json!({ "src": [{ "cidr": "0.0.0.0/0" }] });
        assert!(matches_filter(&policy, &attrs(&[("src", "10.0.0.5")])));
        assert!(matches_filter(&policy, &attrs(&[("src", "192.168.99.42")])));
    }

    #[test]
    fn test_cidr_ipv6_match() {
        let policy = json!({ "src": [{ "cidr": "2001:db8::/32" }] });
        assert!(matches_filter(&policy, &attrs(&[("src", "2001:db8::1")])));
        assert!(!matches_filter(&policy, &attrs(&[("src", "2001:db9::1")])));
        // v4 inside v6 CIDR — no match (mixed family).
        assert!(!matches_filter(&policy, &attrs(&[("src", "10.0.0.1")])));
    }

    #[test]
    fn test_cidr_invalid_prefix_returns_false() {
        let policy = json!({ "src": [{ "cidr": "10.0.0.0/64" }] });
        assert!(!matches_filter(&policy, &attrs(&[("src", "10.0.0.5")])));
    }

    #[test]
    fn test_body_matches_top_level_value() {
        let policy = json!({ "kind": ["order"] });
        let body = json!({ "kind": "order" });
        assert!(matches_filter_body(&policy, &body));
        let body2 = json!({ "kind": "shipment" });
        assert!(!matches_filter_body(&policy, &body2));
    }

    #[test]
    fn test_body_matches_nested_value() {
        let policy = json!({ "detail": { "status": ["FAILED"] } });
        let body = json!({ "detail": { "status": "FAILED" } });
        assert!(matches_filter_body(&policy, &body));
        let body2 = json!({ "detail": { "status": "OK" } });
        assert!(!matches_filter_body(&policy, &body2));
        // Missing nested path → no match.
        let body3 = json!({ "other": { "status": "FAILED" } });
        assert!(!matches_filter_body(&policy, &body3));
    }

    #[test]
    fn test_body_matches_operator_object() {
        let policy = json!({ "url": [{ "prefix": "https://" }] });
        let body = json!({ "url": "https://example.com" });
        assert!(matches_filter_body(&policy, &body));
        let body2 = json!({ "url": "http://example.com" });
        assert!(!matches_filter_body(&policy, &body2));
    }

    #[test]
    fn test_body_matches_numeric_at_nested_path() {
        let policy = json!({ "detail": { "count": [{ "numeric": [">=", 100] }] } });
        let body = json!({ "detail": { "count": 250 } });
        assert!(matches_filter_body(&policy, &body));
        let body2 = json!({ "detail": { "count": 50 } });
        assert!(!matches_filter_body(&policy, &body2));
    }

    #[test]
    fn test_combined_filter_policy() {
        // Like the example in the task specification
        let policy = json!({
            "store": ["example_corp"],
            "event": [{ "prefix": "order-" }],
            "price_usd": [{ "numeric": [">=", 100] }]
        });

        let matching = attrs(&[
            ("store", "example_corp"),
            ("event", "order-created"),
            ("price_usd", "150"),
        ]);
        assert!(matches_filter(&policy, &matching));

        // Wrong store
        let wrong_store = attrs(&[
            ("store", "other_corp"),
            ("event", "order-created"),
            ("price_usd", "150"),
        ]);
        assert!(!matches_filter(&policy, &wrong_store));

        // Price too low
        let cheap = attrs(&[
            ("store", "example_corp"),
            ("event", "order-created"),
            ("price_usd", "50"),
        ]);
        assert!(!matches_filter(&policy, &cheap));

        // Wrong event prefix
        let bad_event = attrs(&[
            ("store", "example_corp"),
            ("event", "invoice-created"),
            ("price_usd", "150"),
        ]);
        assert!(!matches_filter(&policy, &bad_event));
    }
}
