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

fn matches_single_condition(condition: &Value, attr: Option<&Value>) -> bool {
    match condition {
        // String exact match
        Value::String(s) => attr
            .and_then(|a| a["Value"].as_str().or_else(|| a.as_str()))
            .map(|v| v == s)
            .unwrap_or(false),
        // Numeric match
        Value::Number(n) => attr
            .and_then(|a| a["Value"].as_str().or_else(|| a.as_str()))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| Some(v) == n.as_f64())
            .unwrap_or(false),
        // Object conditions (prefix, numeric, exists)
        Value::Object(obj) => {
            if let Some(prefix) = obj.get("prefix").and_then(|v| v.as_str()) {
                return attr
                    .and_then(|a| a["Value"].as_str().or_else(|| a.as_str()))
                    .map(|v| v.starts_with(prefix))
                    .unwrap_or(false);
            }
            if let Some(exists) = obj.get("exists").and_then(|v| v.as_bool()) {
                return attr.is_some() == exists;
            }
            if let Some(numeric) = obj.get("numeric").and_then(|v| v.as_array()) {
                return matches_numeric(numeric, attr);
            }
            false
        }
        _ => false,
    }
}

fn matches_numeric(conditions: &[Value], attr: Option<&Value>) -> bool {
    let val = match attr
        .and_then(|a| a["Value"].as_str().or_else(|| a.as_str()))
        .and_then(|v| v.parse::<f64>().ok())
    {
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
