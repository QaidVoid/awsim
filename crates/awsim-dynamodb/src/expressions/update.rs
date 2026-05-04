use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::DynamoItem;

use super::parser::resolve_path;

/// Apply a DynamoDB UpdateExpression to a mutable item.
///
/// Supports: SET, REMOVE, ADD, DELETE clauses.
pub fn apply_update_expression(
    item: &mut DynamoItem,
    update_expr: &str,
    expr_attr_names: &HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Result<(), AwsError> {
    // Parse update expression into clauses
    let (set_actions, remove_paths, add_actions, delete_actions) =
        parse_update_expression(update_expr)?;

    // Apply SET
    for (path, value_expr) in set_actions {
        let resolved_path = resolve_path(&path, expr_attr_names);
        let value = evaluate_value_expr(&value_expr, item, expr_attr_names, expr_attr_values)?;
        set_nested(item, &resolved_path, value);
    }

    // Apply REMOVE
    for path in remove_paths {
        let resolved_path = resolve_path(&path, expr_attr_names);
        remove_nested(item, &resolved_path);
    }

    // Apply ADD
    for (path, value_expr) in add_actions {
        let resolved_path = resolve_path(&path, expr_attr_names);
        let value = evaluate_value_expr(&value_expr, item, expr_attr_names, expr_attr_values)?;
        apply_add(item, &resolved_path, &value)?;
    }

    // Apply DELETE
    for (path, value_expr) in delete_actions {
        let resolved_path = resolve_path(&path, expr_attr_names);
        let value = evaluate_value_expr(&value_expr, item, expr_attr_names, expr_attr_values)?;
        apply_delete(item, &resolved_path, &value)?;
    }

    Ok(())
}

/// Parsed SET action: (path, value_expression_string)
type SetAction = (String, String);
/// Parsed ADD/DELETE action: (path, value_expression_string)
type AddDeleteAction = (String, String);

/// Parsed clauses of an UpdateExpression: (SET actions, REMOVE paths, ADD actions, DELETE actions).
type UpdateClauses = (
    Vec<SetAction>,
    Vec<String>,
    Vec<AddDeleteAction>,
    Vec<AddDeleteAction>,
);

/// Parse the update expression into its constituent clauses.
fn parse_update_expression(expr: &str) -> Result<UpdateClauses, AwsError> {
    let mut set_actions: Vec<SetAction> = Vec::new();
    let mut remove_paths: Vec<String> = Vec::new();
    let mut add_actions: Vec<AddDeleteAction> = Vec::new();
    let mut delete_actions: Vec<AddDeleteAction> = Vec::new();

    // Split by SET|REMOVE|ADD|DELETE keywords (case-insensitive)
    // Find clause boundaries
    let upper = expr.to_uppercase();
    let mut clauses: Vec<(String, usize)> = Vec::new();

    for kw in &["SET ", "REMOVE ", "ADD ", "DELETE "] {
        let mut start = 0;
        while let Some(pos) = upper[start..].find(kw) {
            let abs_pos = start + pos;
            // Make sure this is a word boundary (not inside a name)
            let is_word_boundary = abs_pos == 0
                || upper
                    .chars()
                    .nth(abs_pos - 1)
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            if is_word_boundary {
                clauses.push((kw.trim().to_string(), abs_pos));
            }
            start = abs_pos + kw.len();
        }
    }

    clauses.sort_by_key(|(_, pos)| *pos);

    for i in 0..clauses.len() {
        let (ref kw, start) = clauses[i];
        let end = if i + 1 < clauses.len() {
            clauses[i + 1].1
        } else {
            expr.len()
        };
        let clause_content = &expr[start + kw.len() + 1..end].trim().to_string();
        let clause_content = clause_content.trim();

        match kw.as_str() {
            "SET" => {
                for action in split_actions(clause_content) {
                    if let Some(eq_pos) = action.find('=') {
                        let path = action[..eq_pos].trim().to_string();
                        let val_expr = action[eq_pos + 1..].trim().to_string();
                        set_actions.push((path, val_expr));
                    }
                }
            }
            "REMOVE" => {
                for path in split_actions(clause_content) {
                    remove_paths.push(path.trim().to_string());
                }
            }
            "ADD" => {
                for action in split_actions(clause_content) {
                    // ADD path value (space-separated)
                    if let Some(space) = action.find(' ') {
                        let path = action[..space].trim().to_string();
                        let val = action[space + 1..].trim().to_string();
                        add_actions.push((path, val));
                    }
                }
            }
            "DELETE" => {
                for action in split_actions(clause_content) {
                    if let Some(space) = action.find(' ') {
                        let path = action[..space].trim().to_string();
                        let val = action[space + 1..].trim().to_string();
                        delete_actions.push((path, val));
                    }
                }
            }
            _ => {}
        }
    }

    Ok((set_actions, remove_paths, add_actions, delete_actions))
}

/// Split comma-separated actions, respecting parentheses.
fn split_actions(s: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();

    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    results.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        results.push(trimmed);
    }
    results
}

/// Evaluate a value expression for SET actions.
/// Supports:
///   - :placeholder
///   - path + :placeholder (numeric addition)
///   - path - :placeholder (numeric subtraction)
///   - if_not_exists(path, :default)
///   - list_append(:list1, :list2)
fn evaluate_value_expr(
    expr: &str,
    item: &DynamoItem,
    expr_attr_names: &HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Result<Value, AwsError> {
    let expr = expr.trim();

    // if_not_exists(path, :default)
    if let Some(args_str) = expr
        .strip_prefix("if_not_exists(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = args_str.splitn(2, ',').collect();
        if parts.len() == 2 {
            let path = resolve_path(parts[0].trim(), expr_attr_names);
            let default_expr = parts[1].trim();
            // If attribute exists, return it; otherwise return default
            if let Some(existing) = get_nested_val(item, &path) {
                return Ok(existing.clone());
            }
            return evaluate_value_expr(default_expr, item, expr_attr_names, expr_attr_values);
        }
    }

    // list_append(:list1, :list2)
    if let Some(args_str) = expr
        .strip_prefix("list_append(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = args_str.splitn(2, ',').collect();
        if parts.len() == 2 {
            let left_val =
                evaluate_value_expr(parts[0].trim(), item, expr_attr_names, expr_attr_values)?;
            let right_val =
                evaluate_value_expr(parts[1].trim(), item, expr_attr_names, expr_attr_values)?;
            let mut combined = Vec::new();
            if let Some(arr) = left_val.get("L").and_then(|v| v.as_array()) {
                combined.extend(arr.clone());
            }
            if let Some(arr) = right_val.get("L").and_then(|v| v.as_array()) {
                combined.extend(arr.clone());
            }
            return Ok(json!({ "L": combined }));
        }
    }

    // Check for arithmetic: path + :val or path - :val
    // Look for + or - at the top level (not inside parens)
    if let Some((left_expr, op, right_expr)) = find_top_level_arithmetic(expr) {
        let left_val = evaluate_value_expr(&left_expr, item, expr_attr_names, expr_attr_values)?;
        let right_val = evaluate_value_expr(&right_expr, item, expr_attr_names, expr_attr_values)?;

        let ln = extract_num(&left_val).unwrap_or(0.0);
        let rn = extract_num(&right_val).unwrap_or(0.0);
        let result = if op == '+' { ln + rn } else { ln - rn };

        // Format number: no trailing .0 if integer
        let s = if result.fract() == 0.0 {
            format!("{}", result as i64)
        } else {
            result.to_string()
        };
        return Ok(json!({ "N": s }));
    }

    // Simple placeholder
    if let Some(placeholder) = expr.strip_prefix(':') {
        let full = format!(":{placeholder}");
        return expr_attr_values.get(&full).cloned().ok_or_else(|| {
            AwsError::validation(format!(
                "Value {full} not found in ExpressionAttributeValues"
            ))
        });
    }

    // Path reference
    let resolved = resolve_path(expr, expr_attr_names);
    if let Some(val) = get_nested_val(item, &resolved) {
        return Ok(val.clone());
    }

    Err(AwsError::validation(format!(
        "Cannot resolve value expression: {expr}"
    )))
}

/// Find a top-level + or - operator in an expression (not inside parens).
fn find_top_level_arithmetic(expr: &str) -> Option<(String, char, String)> {
    let mut depth = 0usize;
    let chars: Vec<char> = expr.chars().collect();

    // Scan from right to left to handle left-associativity
    let mut i = chars.len();
    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                depth = depth.saturating_sub(1);
            }
            '+' | '-' if depth == 0 && i > 0 => {
                // Don't treat unary minus as binary op
                let op = chars[i];
                let left = expr[..i].trim().to_string();
                let right = expr[i + 1..].trim().to_string();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, op, right));
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_num(val: &Value) -> Option<f64> {
    val.get("N")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
}

/// Get a possibly-nested attribute value from an item. Delegates to the
/// shared eval-side path resolver so list indexing (`tags[2].name`) and
/// map traversal stay consistent across condition / projection / update.
fn get_nested_val<'a>(item: &'a DynamoItem, path: &str) -> Option<&'a Value> {
    crate::expressions::eval::get_nested(item, path)
}

/// Set a nested attribute value in an item (dot-path notation).
fn set_nested(item: &mut DynamoItem, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        item.insert(parts[0].to_string(), value);
    } else {
        // For nested paths, build the intermediate M maps
        let entry = item
            .entry(parts[0].to_string())
            .or_insert_with(|| json!({ "M": {} }));
        set_nested_in_value(entry, &parts[1..], value);
    }
}

fn set_nested_in_value(current: &mut Value, parts: &[&str], value: Value) {
    if parts.is_empty() {
        return;
    }
    let map = current
        .as_object_mut()
        .and_then(|o| o.get_mut("M"))
        .and_then(|m| m.as_object_mut());

    if let Some(map) = map {
        if parts.len() == 1 {
            map.insert(parts[0].to_string(), value);
        } else {
            let entry = map
                .entry(parts[0].to_string())
                .or_insert_with(|| json!({ "M": {} }));
            set_nested_in_value(entry, &parts[1..], value);
        }
    }
}

/// Remove a nested attribute from an item.
fn remove_nested(item: &mut DynamoItem, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        item.remove(parts[0]);
    } else {
        if let Some(entry) = item.get_mut(parts[0]) {
            remove_nested_in_value(entry, &parts[1..]);
        }
    }
}

fn remove_nested_in_value(current: &mut Value, parts: &[&str]) {
    if parts.is_empty() {
        return;
    }
    let map = current
        .as_object_mut()
        .and_then(|o| o.get_mut("M"))
        .and_then(|m| m.as_object_mut());
    if let Some(map) = map {
        if parts.len() == 1 {
            map.remove(parts[0]);
        } else if let Some(entry) = map.get_mut(parts[0]) {
            remove_nested_in_value(entry, &parts[1..]);
        }
    }
}

/// Walk a dot-separated map path and run `f` on the leaf value's parent
/// container so the caller can insert / replace / remove the leaf. The
/// intermediate maps are auto-created on missing-segment writes.
fn with_leaf_mut<F>(item: &mut DynamoItem, path: &str, f: F)
where
    F: FnOnce(&mut serde_json::Map<String, Value>, &str),
{
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        // Top-level: synthesize a one-shot Map<String, Value> view.
        // BTreeMap is not serde_json::Map, so we operate directly on `item`.
        let key = parts[0];
        let mut shim = serde_json::Map::new();
        if let Some(v) = item.get(key) {
            shim.insert(key.to_string(), v.clone());
        }
        f(&mut shim, key);
        match shim.remove(key) {
            Some(v) => {
                item.insert(key.to_string(), v);
            }
            None => {
                item.remove(key);
            }
        }
        return;
    }

    let entry = item
        .entry(parts[0].to_string())
        .or_insert_with(|| json!({ "M": {} }));
    descend(entry, &parts[1..], f);

    fn descend<F: FnOnce(&mut serde_json::Map<String, Value>, &str)>(
        current: &mut Value,
        parts: &[&str],
        f: F,
    ) {
        let Some(map) = current
            .as_object_mut()
            .and_then(|o| o.get_mut("M"))
            .and_then(|m| m.as_object_mut())
        else {
            return;
        };
        if parts.len() == 1 {
            f(map, parts[0]);
        } else {
            let next = map
                .entry(parts[0].to_string())
                .or_insert_with(|| json!({ "M": {} }));
            descend(next, &parts[1..], f);
        }
    }
}

/// Format the result of a DynamoDB numeric ADD. Integer-valued floats are
/// emitted without a decimal so SDK clients see canonical "1" rather than
/// "1.0", matching AWS.
fn format_dynamo_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// Set-type tag: SS (string), NS (number), BS (binary).
const SET_TAGS: [&str; 3] = ["SS", "NS", "BS"];

/// Return the AttributeValue tag for a typed-set value (SS/NS/BS) or None.
fn set_tag(v: &Value) -> Option<&'static str> {
    SET_TAGS.iter().copied().find(|&t| v.get(t).is_some())
}

/// Apply ADD operation: numeric increment for N, set union for SS/NS/BS.
fn apply_add(item: &mut DynamoItem, path: &str, value: &Value) -> Result<(), AwsError> {
    with_leaf_mut(item, path, |map, key| {
        let existing = map.get(key).cloned();
        match existing {
            None => {
                map.insert(key.to_string(), value.clone());
            }
            Some(existing) => {
                if let (Some(en), Some(vn)) = (
                    existing.get("N").and_then(|v| v.as_str()),
                    value.get("N").and_then(|v| v.as_str()),
                ) {
                    let result =
                        en.parse::<f64>().unwrap_or(0.0) + vn.parse::<f64>().unwrap_or(0.0);
                    map.insert(
                        key.to_string(),
                        json!({ "N": format_dynamo_number(result) }),
                    );
                } else if let Some(tag) = set_tag(&existing)
                    && value.get(tag).is_some()
                {
                    let mut combined: Vec<Value> = existing
                        .get(tag)
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if let Some(new) = value.get(tag).and_then(|v| v.as_array()) {
                        for v in new {
                            if !combined.contains(v) {
                                combined.push(v.clone());
                            }
                        }
                    }
                    map.insert(key.to_string(), json!({ tag: combined }));
                }
            }
        }
    });
    Ok(())
}

/// Apply DELETE operation (set subtraction). Drops the attribute if the
/// resulting set is empty, matching AWS semantics where empty sets are
/// not allowed to be persisted.
fn apply_delete(item: &mut DynamoItem, path: &str, value: &Value) -> Result<(), AwsError> {
    with_leaf_mut(item, path, |map, key| {
        let Some(existing) = map.get(key).cloned() else {
            return;
        };
        let Some(tag) = set_tag(&existing) else {
            return;
        };
        let arr = existing
            .get(tag)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let to_remove: Vec<&Value> = value
            .get(tag)
            .and_then(|v| v.as_array())
            .map(|v| v.iter().collect())
            .unwrap_or_default();
        let remaining: Vec<Value> = arr
            .into_iter()
            .filter(|v| !to_remove.contains(&v))
            .collect();
        if remaining.is_empty() {
            map.remove(key);
        } else {
            map.insert(key.to_string(), json!({ tag: remaining }));
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn names() -> HashMap<String, String> {
        HashMap::new()
    }

    fn empty_values() -> Map<String, Value> {
        Map::new()
    }

    #[test]
    fn add_unions_binary_set() {
        let mut item = DynamoItem::new();
        item.insert("blobs".into(), json!({ "BS": ["AA==", "AQ=="] }));
        let mut values = Map::new();
        values.insert(":new".into(), json!({ "BS": ["AQ==", "Ag=="] }));
        apply_update_expression(&mut item, "ADD blobs :new", &names(), &values).unwrap();
        let bs = item["blobs"]["BS"].as_array().unwrap();
        let strs: Vec<&str> = bs.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"AA=="));
        assert!(strs.contains(&"AQ=="));
        assert!(strs.contains(&"Ag=="));
        assert_eq!(strs.len(), 3);
    }

    #[test]
    fn delete_removes_attribute_when_set_empties() {
        let mut item = DynamoItem::new();
        item.insert("tags".into(), json!({ "SS": ["a", "b"] }));
        let mut values = Map::new();
        values.insert(":all".into(), json!({ "SS": ["a", "b"] }));
        apply_update_expression(&mut item, "DELETE tags :all", &names(), &values).unwrap();
        assert!(!item.contains_key("tags"));
    }

    #[test]
    fn add_increments_nested_numeric_attribute() {
        let mut item = DynamoItem::new();
        item.insert("stats".into(), json!({ "M": { "count": {"N": "5"} } }));
        let mut values = Map::new();
        values.insert(":n".into(), json!({ "N": "3" }));
        apply_update_expression(&mut item, "ADD stats.count :n", &names(), &values).unwrap();
        assert_eq!(item["stats"]["M"]["count"]["N"], json!("8"));
    }

    #[test]
    fn add_creates_nested_attribute_when_missing() {
        let mut item = DynamoItem::new();
        let mut values = Map::new();
        values.insert(":n".into(), json!({ "N": "1" }));
        apply_update_expression(&mut item, "ADD stats.count :n", &names(), &values).unwrap();
        assert_eq!(item["stats"]["M"]["count"]["N"], json!("1"));
    }

    #[test]
    fn delete_subtracts_from_nested_string_set() {
        let mut item = DynamoItem::new();
        item.insert(
            "user".into(),
            json!({ "M": { "tags": { "SS": ["a", "b", "c"] } } }),
        );
        let mut values = Map::new();
        values.insert(":rm".into(), json!({ "SS": ["b"] }));
        apply_update_expression(&mut item, "DELETE user.tags :rm", &names(), &values).unwrap();
        let tags = item["user"]["M"]["tags"]["SS"].as_array().unwrap();
        let strs: Vec<&str> = tags.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(strs, ["a", "c"]);
    }

    #[test]
    fn add_integer_result_omits_decimal() {
        let mut item = DynamoItem::new();
        item.insert("n".into(), json!({ "N": "1.5" }));
        let mut values = empty_values();
        values.insert(":d".into(), json!({ "N": "0.5" }));
        apply_update_expression(&mut item, "ADD n :d", &names(), &values).unwrap();
        assert_eq!(item["n"]["N"], json!("2"));
    }
}
