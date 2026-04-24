use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::Value;

use crate::state::DynamoItem;

use super::parser::{CompareOp, ConditionExpr, Operand, resolve_path, resolve_value};

/// Evaluate a condition expression against an item.
/// Returns Ok(true) if the condition is satisfied, Ok(false) otherwise.
pub fn evaluate_condition(
    expr: &ConditionExpr,
    item: &DynamoItem,
    expr_attr_names: &HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Result<bool, AwsError> {
    match expr {
        ConditionExpr::Comparison { left, op, right } => {
            let lv = resolve_operand(left, item, expr_attr_names, expr_attr_values);
            let rv = resolve_operand(right, item, expr_attr_names, expr_attr_values);
            match (lv, rv) {
                (Some(l), Some(r)) => Ok(compare_values(l, op, r)),
                // If either side is missing, comparison is false (except Ne which would be true)
                _ => Ok(*op == CompareOp::Ne),
            }
        }

        ConditionExpr::Between { operand, low, high } => {
            let v = resolve_operand(operand, item, expr_attr_names, expr_attr_values);
            let lo = resolve_operand(low, item, expr_attr_names, expr_attr_values);
            let hi = resolve_operand(high, item, expr_attr_names, expr_attr_values);
            match (v, lo, hi) {
                (Some(v), Some(lo), Some(hi)) => {
                    Ok(compare_values(v, &CompareOp::Ge, lo)
                        && compare_values(v, &CompareOp::Le, hi))
                }
                _ => Ok(false),
            }
        }

        ConditionExpr::In { operand, values } => {
            let v = resolve_operand(operand, item, expr_attr_names, expr_attr_values);
            match v {
                None => Ok(false),
                Some(v) => {
                    for candidate in values {
                        if let Some(c) =
                            resolve_operand(candidate, item, expr_attr_names, expr_attr_values)
                            && dynamo_values_equal(v, c) {
                                return Ok(true);
                            }
                    }
                    Ok(false)
                }
            }
        }

        ConditionExpr::Logical { op, children } => match op {
            super::parser::LogicalOp::And => {
                for child in children {
                    if !evaluate_condition(child, item, expr_attr_names, expr_attr_values)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            super::parser::LogicalOp::Or => {
                for child in children {
                    if evaluate_condition(child, item, expr_attr_names, expr_attr_values)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        },

        ConditionExpr::Not(inner) => Ok(!evaluate_condition(
            inner,
            item,
            expr_attr_names,
            expr_attr_values,
        )?),

        ConditionExpr::AttributeExists(path) => {
            let resolved = resolve_path(path, expr_attr_names);
            Ok(get_nested(item, &resolved).is_some())
        }

        ConditionExpr::AttributeNotExists(path) => {
            let resolved = resolve_path(path, expr_attr_names);
            Ok(get_nested(item, &resolved).is_none())
        }

        ConditionExpr::BeginsWith(path_op, val_op) => {
            let path_val = resolve_operand(path_op, item, expr_attr_names, expr_attr_values);
            let prefix_val = resolve_operand(val_op, item, expr_attr_names, expr_attr_values);
            match (path_val, prefix_val) {
                (Some(pv), Some(prefix)) => {
                    let s = extract_string_from_dynamo(pv);
                    let p = extract_string_from_dynamo(prefix);
                    match (s, p) {
                        (Some(sv), Some(pv)) => Ok(sv.starts_with(pv.as_str())),
                        _ => Ok(false),
                    }
                }
                _ => Ok(false),
            }
        }

        ConditionExpr::Contains(path_op, val_op) => {
            let path_val = resolve_operand(path_op, item, expr_attr_names, expr_attr_values);
            let needle_val = resolve_operand(val_op, item, expr_attr_names, expr_attr_values);
            match (path_val, needle_val) {
                (Some(container), Some(needle)) => Ok(dynamo_contains(container, needle)),
                _ => Ok(false),
            }
        }

        ConditionExpr::SizeComparison { path, op, right } => {
            let resolved = resolve_path(path, expr_attr_names);
            let attr_val = get_nested(item, &resolved);
            let right_val = resolve_operand(right, item, expr_attr_names, expr_attr_values);
            match (attr_val, right_val) {
                (Some(av), Some(rv)) => {
                    let sz = dynamo_size(av) as i64;
                    let rn = extract_number_from_dynamo(rv).unwrap_or(0.0) as i64;
                    Ok(match op {
                        CompareOp::Eq => sz == rn,
                        CompareOp::Ne => sz != rn,
                        CompareOp::Lt => sz < rn,
                        CompareOp::Le => sz <= rn,
                        CompareOp::Gt => sz > rn,
                        CompareOp::Ge => sz >= rn,
                    })
                }
                _ => Ok(false),
            }
        }
    }
}

// ─── Helper functions ─────────────────────────────────────────────────────────

/// Resolve an Operand to a DynamoDB value reference.
fn resolve_operand<'a>(
    operand: &Operand,
    item: &'a DynamoItem,
    expr_attr_names: &HashMap<String, String>,
    expr_attr_values: &'a serde_json::Map<String, Value>,
) -> Option<&'a Value> {
    match operand {
        Operand::Path(path) => {
            let resolved = resolve_path(path, expr_attr_names);
            get_nested(item, &resolved)
        }
        Operand::Value(placeholder) => resolve_value(placeholder, expr_attr_values),
    }
}

/// Get a possibly-nested attribute from an item, using dot notation.
pub fn get_nested<'a>(item: &'a DynamoItem, path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current: Option<&Value> = item.get(parts[0]);
    for part in &parts[1..] {
        current = current.and_then(|v| {
            // DynamoDB Map type: {"M": {"key": {...}}}
            if let Some(map) = v.get("M").and_then(|m| m.as_object()) {
                map.get(*part)
            } else {
                None
            }
        });
    }
    current
}

/// Compare two DynamoDB typed values with a comparison operator.
fn compare_values(left: &Value, op: &CompareOp, right: &Value) -> bool {
    // Numeric comparison if both N
    if let (Some(ln), Some(rn)) = (
        left.get("N").and_then(|v| v.as_str()),
        right.get("N").and_then(|v| v.as_str()),
    ) {
        let lf: f64 = ln.parse().unwrap_or(0.0);
        let rf: f64 = rn.parse().unwrap_or(0.0);
        return match op {
            CompareOp::Eq => (lf - rf).abs() < f64::EPSILON,
            CompareOp::Ne => (lf - rf).abs() >= f64::EPSILON,
            CompareOp::Lt => lf < rf,
            CompareOp::Le => lf <= rf,
            CompareOp::Gt => lf > rf,
            CompareOp::Ge => lf >= rf,
        };
    }

    // String comparison
    if let (Some(ls), Some(rs)) = (
        left.get("S").and_then(|v| v.as_str()),
        right.get("S").and_then(|v| v.as_str()),
    ) {
        return match op {
            CompareOp::Eq => ls == rs,
            CompareOp::Ne => ls != rs,
            CompareOp::Lt => ls < rs,
            CompareOp::Le => ls <= rs,
            CompareOp::Gt => ls > rs,
            CompareOp::Ge => ls >= rs,
        };
    }

    // Boolean comparison
    if let (Some(lb), Some(rb)) = (left.get("BOOL"), right.get("BOOL")) {
        let lb = lb.as_bool().unwrap_or(false);
        let rb = rb.as_bool().unwrap_or(false);
        return match op {
            CompareOp::Eq => lb == rb,
            CompareOp::Ne => lb != rb,
            _ => false,
        };
    }

    // NULL comparison
    if left.get("NULL").is_some() && right.get("NULL").is_some() {
        return matches!(op, CompareOp::Eq);
    }

    // Default: equality check by JSON value
    match op {
        CompareOp::Eq => dynamo_values_equal(left, right),
        CompareOp::Ne => !dynamo_values_equal(left, right),
        _ => false,
    }
}

fn dynamo_values_equal(a: &Value, b: &Value) -> bool {
    a == b
}

fn extract_string_from_dynamo(val: &Value) -> Option<String> {
    if let Some(s) = val.get("S").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    if let Some(n) = val.get("N").and_then(|v| v.as_str()) {
        return Some(n.to_string());
    }
    None
}

fn extract_number_from_dynamo(val: &Value) -> Option<f64> {
    val.get("N")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
}

fn dynamo_size(val: &Value) -> usize {
    if let Some(s) = val.get("S").and_then(|v| v.as_str()) {
        return s.len();
    }
    if let Some(n) = val.get("N").and_then(|v| v.as_str()) {
        return n.len();
    }
    if let Some(b) = val.get("B").and_then(|v| v.as_str()) {
        return b.len();
    }
    if let Some(arr) = val.get("L").and_then(|v| v.as_array()) {
        return arr.len();
    }
    if let Some(map) = val.get("M").and_then(|v| v.as_object()) {
        return map.len();
    }
    if let Some(ss) = val.get("SS").and_then(|v| v.as_array()) {
        return ss.len();
    }
    if let Some(ns) = val.get("NS").and_then(|v| v.as_array()) {
        return ns.len();
    }
    if let Some(bs) = val.get("BS").and_then(|v| v.as_array()) {
        return bs.len();
    }
    0
}

fn dynamo_contains(container: &Value, needle: &Value) -> bool {
    // String contains substring
    if let (Some(cs), Some(ns)) = (
        container.get("S").and_then(|v| v.as_str()),
        needle.get("S").and_then(|v| v.as_str()),
    ) {
        return cs.contains(ns);
    }
    // List contains item
    if let Some(arr) = container.get("L").and_then(|v| v.as_array()) {
        return arr.iter().any(|el| dynamo_values_equal(el, needle));
    }
    // Set contains value
    if let Some(ss) = container.get("SS").and_then(|v| v.as_array())
        && let Some(ns) = needle.get("S").and_then(|v| v.as_str()) {
            return ss.iter().any(|el| el.as_str() == Some(ns));
        }
    if let Some(ns_arr) = container.get("NS").and_then(|v| v.as_array())
        && let Some(n) = needle.get("N").and_then(|v| v.as_str()) {
            return ns_arr.iter().any(|el| el.as_str() == Some(n));
        }
    false
}
