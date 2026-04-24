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

/// Parse the update expression into its constituent clauses.
fn parse_update_expression(
    expr: &str,
) -> Result<
    (
        Vec<SetAction>,
        Vec<String>,
        Vec<AddDeleteAction>,
        Vec<AddDeleteAction>,
    ),
    AwsError,
> {
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

/// Get a possibly-nested attribute value from an item.
fn get_nested_val<'a>(item: &'a DynamoItem, path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current: Option<&Value> = item.get(parts[0]);
    for part in &parts[1..] {
        current = current.and_then(|v| {
            v.get("M")
                .and_then(|m| m.as_object())
                .and_then(|m| m.get(*part))
        });
    }
    current
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

/// Apply ADD operation (numeric increment or set union).
fn apply_add(item: &mut DynamoItem, path: &str, value: &Value) -> Result<(), AwsError> {
    let existing = item.get(path).cloned();
    match existing {
        None => {
            // If no existing value, just set it
            item.insert(path.to_string(), value.clone());
        }
        Some(existing) => {
            if let (Some(en), Some(vn)) = (
                existing.get("N").and_then(|v| v.as_str()),
                value.get("N").and_then(|v| v.as_str()),
            ) {
                let result: f64 =
                    en.parse::<f64>().unwrap_or(0.0) + vn.parse::<f64>().unwrap_or(0.0);
                let s = if result.fract() == 0.0 {
                    format!("{}", result as i64)
                } else {
                    result.to_string()
                };
                item.insert(path.to_string(), json!({ "N": s }));
            } else if existing.get("SS").is_some() {
                // Union of sets
                let mut ss: Vec<Value> = existing
                    .get("SS")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if let Some(new_ss) = value.get("SS").and_then(|v| v.as_array()) {
                    for v in new_ss {
                        if !ss.contains(v) {
                            ss.push(v.clone());
                        }
                    }
                }
                item.insert(path.to_string(), json!({ "SS": ss }));
            } else if existing.get("NS").is_some() {
                let mut ns: Vec<Value> = existing
                    .get("NS")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if let Some(new_ns) = value.get("NS").and_then(|v| v.as_array()) {
                    for v in new_ns {
                        if !ns.contains(v) {
                            ns.push(v.clone());
                        }
                    }
                }
                item.insert(path.to_string(), json!({ "NS": ns }));
            }
        }
    }
    Ok(())
}

/// Apply DELETE operation (set subtraction).
fn apply_delete(item: &mut DynamoItem, path: &str, value: &Value) -> Result<(), AwsError> {
    if let Some(existing) = item.get(path).cloned() {
        if let Some(ss) = existing.get("SS").and_then(|v| v.as_array()) {
            let to_remove: Vec<&Value> = value
                .get("SS")
                .and_then(|v| v.as_array())
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            let new_ss: Vec<Value> = ss
                .iter()
                .filter(|v| !to_remove.contains(v))
                .cloned()
                .collect();
            item.insert(path.to_string(), json!({ "SS": new_ss }));
        } else if let Some(ns) = existing.get("NS").and_then(|v| v.as_array()) {
            let to_remove: Vec<&Value> = value
                .get("NS")
                .and_then(|v| v.as_array())
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            let new_ns: Vec<Value> = ns
                .iter()
                .filter(|v| !to_remove.contains(v))
                .cloned()
                .collect();
            item.insert(path.to_string(), json!({ "NS": new_ns }));
        }
    }
    Ok(())
}
