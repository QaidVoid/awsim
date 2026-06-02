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
                            && dynamo_values_equal(v, c)
                        {
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
            let resolved = resolve_path(path, expr_attr_names)?;
            Ok(get_nested(item, &resolved).is_some())
        }

        ConditionExpr::AttributeNotExists(path) => {
            let resolved = resolve_path(path, expr_attr_names)?;
            Ok(get_nested(item, &resolved).is_none())
        }

        ConditionExpr::AttributeType(path, type_op) => {
            let resolved = resolve_path(path, expr_attr_names)?;
            let attr = match get_nested(item, &resolved) {
                Some(v) => v,
                None => return Ok(false),
            };
            let type_val = match resolve_operand(type_op, item, expr_attr_names, expr_attr_values) {
                Some(v) => v,
                None => return Ok(false),
            };
            // The type identifier arrives as a DynamoDB S-typed value:
            // {"S": "S"} for string, {"S": "N"} for number, etc. The
            // attribute itself is stored as a one-key object whose key is
            // the type discriminator (S, N, B, BOOL, NULL, L, M, SS, NS,
            // BS), so we just compare them.
            let Some(want) = type_val.get("S").and_then(|v| v.as_str()) else {
                return Ok(false);
            };
            let actual = attr
                .as_object()
                .and_then(|m| m.keys().next().map(String::from))
                .unwrap_or_default();
            Ok(actual == want)
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
            let resolved = resolve_path(path, expr_attr_names)?;
            let attr_val = get_nested(item, &resolved);
            let right_val = resolve_operand(right, item, expr_attr_names, expr_attr_values);
            match (attr_val, right_val) {
                (Some(av), Some(rv)) => {
                    let sz = dynamo_size(av) as i64;
                    let rn = extract_number_from_dynamo(rv).unwrap_or(0);
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
            // A path past the 64 KB limit can never match a stored
            // attribute, so treat the over-long case as "not found"
            // rather than threading a Result through this Option API.
            let resolved = resolve_path(path, expr_attr_names).ok()?;
            get_nested(item, &resolved)
        }
        Operand::Value(placeholder) => resolve_value(placeholder, expr_attr_values),
    }
}

/// One step in an attribute path: a named map key or a list index.
enum PathStep<'a> {
    Name(&'a str),
    Index(usize),
}

/// Tokenize a DynamoDB document path like `tags[2].name[0]` into named
/// segments and list indices. Returns None if the bracket syntax is
/// malformed.
fn tokenize_path(path: &str) -> Option<Vec<PathStep<'_>>> {
    let mut steps: Vec<PathStep<'_>> = Vec::new();
    for segment in path.split('.') {
        // Each dot-segment is either bare (`name`) or has trailing list
        // indices (`name[0]`, `name[0][1]`). Find the first `[` to split
        // the name from the index list.
        let (name, rest) = match segment.find('[') {
            Some(i) => (&segment[..i], &segment[i..]),
            None => (segment, ""),
        };
        if name.is_empty() {
            return None;
        }
        steps.push(PathStep::Name(name));
        let mut cursor = rest;
        while !cursor.is_empty() {
            let close = cursor.find(']')?;
            let idx: usize = cursor[1..close].parse().ok()?;
            steps.push(PathStep::Index(idx));
            cursor = &cursor[close + 1..];
        }
    }
    Some(steps)
}

/// Get a possibly-nested attribute from an item using DynamoDB document
/// path syntax. Supports map traversal (`a.b.c`) and list indexing
/// (`a[0]`, `tags[2].name`).
pub fn get_nested<'a>(item: &'a DynamoItem, path: &str) -> Option<&'a Value> {
    let steps = tokenize_path(path)?;
    let mut iter = steps.into_iter();
    // First step must be an attribute name on the item itself.
    let first = match iter.next()? {
        PathStep::Name(n) => n,
        PathStep::Index(_) => return None,
    };
    let mut current: Option<&Value> = item.get(first);
    for step in iter {
        current = current.and_then(|v| match step {
            PathStep::Name(n) => v
                .get("M")
                .and_then(|m| m.as_object())
                .and_then(|m| m.get(n)),
            PathStep::Index(i) => v.get("L").and_then(|l| l.as_array()).and_then(|a| a.get(i)),
        });
    }
    current
}

/// Compare two DynamoDB typed values with a comparison operator.
fn compare_values(left: &Value, op: &CompareOp, right: &Value) -> bool {
    // Numeric comparison if both N. DynamoDB stores numbers as variable-precision
    // decimals (up to 38 significant digits), so f64's ~15-digit mantissa is not
    // enough: comparing 9_999_999_999_999_999 to 10_000_000_000_000_000 as f64
    // would say they are equal. rust_decimal carries ~28-29 significant digits
    // exactly, which covers timestamps in nanos, IDs above 2^53, and money in
    // millicents — every real DDB workload we have seen. Numbers that fail to
    // parse (malformed wire input) compare as not-equal.
    if let (Some(ln), Some(rn)) = (
        left.get("N").and_then(|v| v.as_str()),
        right.get("N").and_then(|v| v.as_str()),
    ) {
        use std::str::FromStr;
        match (
            rust_decimal::Decimal::from_str(ln),
            rust_decimal::Decimal::from_str(rn),
        ) {
            (Ok(l), Ok(r)) => {
                return match op {
                    CompareOp::Eq => l == r,
                    CompareOp::Ne => l != r,
                    CompareOp::Lt => l < r,
                    CompareOp::Le => l <= r,
                    CompareOp::Gt => l > r,
                    CompareOp::Ge => l >= r,
                };
            }
            _ => {
                // Fall back to literal string equality so equal stringy
                // numbers still compare equal even when out of decimal range.
                return match op {
                    CompareOp::Eq => ln == rn,
                    CompareOp::Ne => ln != rn,
                    _ => false,
                };
            }
        }
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

/// Extract a DynamoDB number as i64. Used only by `size()` comparisons,
/// where the right-hand side is a small whole-number byte count — losing
/// fractional precision is fine, and clipping to i64 range is safer than
/// silently wrapping. Returns None when the value isn't numeric or doesn't
/// fit in i64.
fn extract_number_from_dynamo(val: &Value) -> Option<i64> {
    use std::str::FromStr;
    let s = val.get("N").and_then(|v| v.as_str())?;
    rust_decimal::Decimal::from_str(s)
        .ok()
        .and_then(|d| <i64 as TryFrom<rust_decimal::Decimal>>::try_from(d).ok())
}

fn dynamo_size(val: &Value) -> usize {
    if let Some(s) = val.get("S").and_then(|v| v.as_str()) {
        return s.len();
    }
    if let Some(n) = val.get("N").and_then(|v| v.as_str()) {
        return n.len();
    }
    if let Some(b) = val.get("B").and_then(|v| v.as_str()) {
        // Binary attributes are stored base64-encoded on the wire, but
        // size(B) is documented as the decoded byte count. Approximate the
        // decoded length from the base64 string without allocating: every
        // 4 base64 chars decode to 3 bytes, minus 1 byte per `=` pad.
        let pad = b.bytes().rev().take_while(|&c| c == b'=').count();
        let n = b.len();
        if n % 4 != 0 {
            // Malformed base64 — fall back to actually decoding.
            use base64::Engine as _;
            return base64::engine::general_purpose::STANDARD
                .decode(b)
                .map(|bytes| bytes.len())
                .unwrap_or(0);
        }
        return n / 4 * 3 - pad;
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
        && let Some(ns) = needle.get("S").and_then(|v| v.as_str())
    {
        return ss.iter().any(|el| el.as_str() == Some(ns));
    }
    if let Some(ns_arr) = container.get("NS").and_then(|v| v.as_array())
        && let Some(n) = needle.get("N").and_then(|v| v.as_str())
    {
        return ns_arr.iter().any(|el| el.as_str() == Some(n));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::json;

    #[test]
    fn high_precision_numbers_compare_distinctly() {
        // 16 nines vs 17 nines: f64 cannot tell these apart, but DDB
        // promises 38 sig digits. Without rust_decimal, both `Eq` and
        // `Lt` would lie here.
        let a = json!({ "N": "9999999999999999" });
        let b = json!({ "N": "99999999999999999" });
        assert!(!compare_values(&a, &CompareOp::Eq, &b));
        assert!(compare_values(&a, &CompareOp::Lt, &b));
        assert!(compare_values(&b, &CompareOp::Gt, &a));
    }

    #[test]
    fn whole_number_decimals_compare_equal() {
        // Trailing zero / decimal-vs-int formatting must not change
        // numeric equality.
        let a = json!({ "N": "1.0" });
        let b = json!({ "N": "1" });
        assert!(compare_values(&a, &CompareOp::Eq, &b));
    }

    #[test]
    fn size_of_string_returns_utf8_byte_length() {
        assert_eq!(dynamo_size(&json!({ "S": "hello" })), 5);
        // 4 emoji each 4 UTF-8 bytes wide.
        assert_eq!(dynamo_size(&json!({ "S": "🦀🦀" })), 8);
    }

    #[test]
    fn size_of_binary_returns_decoded_byte_count() {
        // 4 raw bytes → "AAECAw==" (8 chars, 2 padding) → decoded length 4.
        let b = BASE64.encode([0u8, 1, 2, 3]);
        assert_eq!(b.len(), 8);
        assert_eq!(dynamo_size(&json!({ "B": b })), 4);

        // 5 raw bytes → "AAECAwQ=" (8 chars, 1 padding) → decoded length 5.
        let b = BASE64.encode([0u8, 1, 2, 3, 4]);
        assert_eq!(dynamo_size(&json!({ "B": b })), 5);

        // 6 raw bytes → "AAECAwQF" (8 chars, 0 padding) → decoded length 6.
        let b = BASE64.encode([0u8, 1, 2, 3, 4, 5]);
        assert_eq!(dynamo_size(&json!({ "B": b })), 6);

        // Empty binary → "" → 0 bytes.
        assert_eq!(dynamo_size(&json!({ "B": "" })), 0);
    }

    #[test]
    fn size_of_malformed_binary_falls_back_to_decode() {
        // Length not a multiple of 4 — decode-or-zero path.
        assert_eq!(dynamo_size(&json!({ "B": "abc" })), 0);
    }

    #[test]
    fn get_nested_indexes_into_list() {
        let mut item = DynamoItem::new();
        item.insert(
            "tags".into(),
            json!({ "L": [ {"S": "a"}, {"S": "b"}, {"S": "c"} ] }),
        );
        assert_eq!(get_nested(&item, "tags[0]"), Some(&json!({"S": "a"})));
        assert_eq!(get_nested(&item, "tags[2]"), Some(&json!({"S": "c"})));
        // Out-of-bounds resolves to None, never panics.
        assert!(get_nested(&item, "tags[5]").is_none());
    }

    #[test]
    fn get_nested_indexes_into_list_of_maps_then_map_key() {
        let mut item = DynamoItem::new();
        item.insert(
            "friends".into(),
            json!({ "L": [
                { "M": { "name": {"S": "alice"} } },
                { "M": { "name": {"S": "bob"} } },
            ] }),
        );
        assert_eq!(
            get_nested(&item, "friends[1].name"),
            Some(&json!({"S": "bob"}))
        );
    }

    #[test]
    fn get_nested_handles_chained_indices() {
        let mut item = DynamoItem::new();
        item.insert(
            "matrix".into(),
            json!({ "L": [
                { "L": [ {"N": "1"}, {"N": "2"} ] },
                { "L": [ {"N": "3"}, {"N": "4"} ] },
            ] }),
        );
        assert_eq!(get_nested(&item, "matrix[1][0]"), Some(&json!({"N": "3"})));
    }

    #[test]
    fn get_nested_rejects_malformed_bracket_syntax() {
        let mut item = DynamoItem::new();
        item.insert("tags".into(), json!({ "L": [ {"S": "a"} ] }));
        // No closing bracket → unparseable.
        assert!(get_nested(&item, "tags[0").is_none());
        // Non-numeric index → unparseable.
        assert!(get_nested(&item, "tags[a]").is_none());
    }
}
