//! Translation of legacy (pre-expression) request parameters into their
//! modern expression equivalents.
//!
//! Older SDKs and some Terraform providers still send the parameter shapes
//! that predate expression strings: `Expected` / `ConditionalOperator`,
//! `AttributeUpdates`, `KeyConditions`, `QueryFilter`, and `ScanFilter`.
//! Rather than teach every operation two code paths, [`rewrite`] converts the
//! legacy shapes into the equivalent `ConditionExpression`,
//! `UpdateExpression`, `KeyConditionExpression`, or `FilterExpression` (with
//! generated `ExpressionAttributeNames` / `ExpressionAttributeValues`), so the
//! rest of the pipeline only ever deals with expressions.

use std::borrow::Cow;
use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Map, Value};

/// Placeholder prefixes for synthesized names and values. They are distinctive
/// enough not to collide with caller-supplied placeholders when a request
/// mixes a legacy parameter with a modern expression.
const NAME_PREFIX: &str = "#_le_n";
const VALUE_PREFIX: &str = ":_le_v";

/// Legacy parameter names this module knows how to translate.
const LEGACY_KEYS: [&str; 5] = [
    "Expected",
    "AttributeUpdates",
    "KeyConditions",
    "QueryFilter",
    "ScanFilter",
];

/// Rewrite any legacy parameters in `input` into modern expressions.
///
/// Returns `Cow::Borrowed` unchanged when no legacy parameter is present (the
/// common case) and `Cow::Owned` with the translated request otherwise.
///
/// Translations performed:
/// - `Expected` (joined by `ConditionalOperator`) into `ConditionExpression`.
/// - `AttributeUpdates` into `UpdateExpression`.
/// - `KeyConditions` into `KeyConditionExpression`.
/// - `QueryFilter` / `ScanFilter` (joined by `ConditionalOperator`) into
///   `FilterExpression`.
///
/// Supplying a legacy parameter together with its modern counterpart is
/// rejected with `ValidationException`, matching AWS.
pub fn rewrite(input: &Value) -> Result<Cow<'_, Value>, AwsError> {
    let has_legacy = input
        .as_object()
        .is_some_and(|o| LEGACY_KEYS.iter().any(|k| o.contains_key(*k)));
    if !has_legacy {
        return Ok(Cow::Borrowed(input));
    }

    let mut owned = input.clone();
    let obj = owned.as_object_mut().expect("has_legacy implies an object");
    let mut builder = ExprBuilder::new();

    translate_expected(obj, &mut builder)?;
    translate_attribute_updates(obj, &mut builder)?;
    translate_key_conditions(obj, &mut builder)?;
    translate_filter(obj, "QueryFilter", &mut builder)?;
    translate_filter(obj, "ScanFilter", &mut builder)?;

    builder.commit(obj);
    Ok(Cow::Owned(owned))
}

/// Accumulates generated `ExpressionAttributeNames` / `ExpressionAttributeValues`
/// with fresh, collision-free placeholders during translation.
struct ExprBuilder {
    names: Map<String, Value>,
    values: Map<String, Value>,
    /// Attribute name to its `#`-alias, so a repeated attribute reuses one
    /// placeholder.
    aliases: HashMap<String, String>,
    next_value: usize,
}

impl ExprBuilder {
    fn new() -> Self {
        Self {
            names: Map::new(),
            values: Map::new(),
            aliases: HashMap::new(),
            next_value: 0,
        }
    }

    /// Return the `#`-alias for `attr`, allocating one on first use. Aliasing
    /// every attribute name sidesteps reserved-keyword collisions for free.
    fn name(&mut self, attr: &str) -> String {
        if let Some(alias) = self.aliases.get(attr) {
            return alias.clone();
        }
        let alias = format!("{NAME_PREFIX}{}", self.aliases.len());
        self.names
            .insert(alias.clone(), Value::String(attr.to_string()));
        self.aliases.insert(attr.to_string(), alias.clone());
        alias
    }

    /// Allocate a fresh placeholder bound to `value`.
    fn value(&mut self, value: &Value) -> String {
        let placeholder = format!("{VALUE_PREFIX}{}", self.next_value);
        self.next_value += 1;
        self.values.insert(placeholder.clone(), value.clone());
        placeholder
    }

    /// Merge the generated names and values into the request, creating the
    /// `ExpressionAttribute*` maps if the caller did not supply them.
    fn commit(self, obj: &mut Map<String, Value>) {
        if !self.names.is_empty() {
            let entry = obj
                .entry("ExpressionAttributeNames")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(map) = entry.as_object_mut() {
                map.extend(self.names);
            }
        }
        if !self.values.is_empty() {
            let entry = obj
                .entry("ExpressionAttributeValues")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(map) = entry.as_object_mut() {
                map.extend(self.values);
            }
        }
    }
}

fn mutually_exclusive(legacy: &str, modern: &str) -> AwsError {
    AwsError::validation(format!(
        "Cannot specify both {legacy} and {modern}; use {modern}, which \
         supersedes the legacy {legacy} parameter."
    ))
}

/// The `AND` / `OR` connective for `Expected` and the query/scan filters.
/// Defaults to `AND`, matching AWS.
fn conditional_operator(obj: &Map<String, Value>) -> &'static str {
    match obj.get("ConditionalOperator").and_then(Value::as_str) {
        Some("OR") => "OR",
        _ => "AND",
    }
}

/// Build one boolean fragment for a legacy `ComparisonOperator` + value list.
fn comparison_fragment(
    builder: &mut ExprBuilder,
    name: &str,
    op: &str,
    list: &[Value],
) -> Result<String, AwsError> {
    let one = |builder: &mut ExprBuilder, list: &[Value]| -> Result<String, AwsError> {
        require_len(op, list, 1)?;
        Ok(builder.value(&list[0]))
    };
    match op {
        "EQ" => Ok(format!("{name} = {}", one(builder, list)?)),
        "NE" => Ok(format!("{name} <> {}", one(builder, list)?)),
        "LE" => Ok(format!("{name} <= {}", one(builder, list)?)),
        "LT" => Ok(format!("{name} < {}", one(builder, list)?)),
        "GE" => Ok(format!("{name} >= {}", one(builder, list)?)),
        "GT" => Ok(format!("{name} > {}", one(builder, list)?)),
        "BEGINS_WITH" => Ok(format!("begins_with({name}, {})", one(builder, list)?)),
        "CONTAINS" => Ok(format!("contains({name}, {})", one(builder, list)?)),
        "NOT_CONTAINS" => Ok(format!("NOT contains({name}, {})", one(builder, list)?)),
        "BETWEEN" => {
            require_len(op, list, 2)?;
            let low = builder.value(&list[0]);
            let high = builder.value(&list[1]);
            Ok(format!("{name} BETWEEN {low} AND {high}"))
        }
        "IN" => {
            if list.is_empty() {
                return Err(AwsError::validation(
                    "One or more parameter values were invalid: \
                     ComparisonOperator IN requires at least one value.",
                ));
            }
            let placeholders: Vec<String> = list.iter().map(|v| builder.value(v)).collect();
            Ok(format!("{name} IN ({})", placeholders.join(", ")))
        }
        "NULL" => {
            require_len(op, list, 0)?;
            Ok(format!("attribute_not_exists({name})"))
        }
        "NOT_NULL" => {
            require_len(op, list, 0)?;
            Ok(format!("attribute_exists({name})"))
        }
        other => Err(AwsError::validation(format!(
            "Unsupported ComparisonOperator: {other}"
        ))),
    }
}

fn require_len(op: &str, list: &[Value], n: usize) -> Result<(), AwsError> {
    if list.len() == n {
        Ok(())
    } else {
        Err(AwsError::validation(format!(
            "One or more parameter values were invalid: \
             ComparisonOperator {op} requires {n} value(s) in AttributeValueList, got {}.",
            list.len()
        )))
    }
}

fn translate_expected(
    obj: &mut Map<String, Value>,
    builder: &mut ExprBuilder,
) -> Result<(), AwsError> {
    let Some(expected) = obj.remove("Expected") else {
        return Ok(());
    };
    if obj.contains_key("ConditionExpression") {
        return Err(mutually_exclusive("Expected", "ConditionExpression"));
    }
    let conj = conditional_operator(obj);
    let map = expected
        .as_object()
        .ok_or_else(|| AwsError::validation("Expected must be a map of attribute conditions."))?;

    let mut fragments = Vec::with_capacity(map.len());
    for (attr, cond) in map {
        fragments.push(expected_fragment(builder, attr, cond)?);
    }
    if fragments.is_empty() {
        return Ok(());
    }
    obj.insert(
        "ConditionExpression".to_string(),
        Value::String(fragments.join(&format!(" {conj} "))),
    );
    Ok(())
}

/// Translate one `Expected` entry. Supports both the `ComparisonOperator` form
/// and the older `Value` / `Exists` form.
fn expected_fragment(
    builder: &mut ExprBuilder,
    attr: &str,
    cond: &Value,
) -> Result<String, AwsError> {
    let cond = cond
        .as_object()
        .ok_or_else(|| AwsError::validation("Each Expected entry must be a map."))?;
    let name = builder.name(attr);

    if let Some(op) = cond.get("ComparisonOperator").and_then(Value::as_str) {
        let list = cond
            .get("AttributeValueList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        return comparison_fragment(builder, &name, op, &list);
    }

    let exists = cond.get("Exists").and_then(Value::as_bool);
    let value = cond.get("Value");
    match (exists, value) {
        (Some(false), None) => Ok(format!("attribute_not_exists({name})")),
        (Some(false), Some(_)) => Err(AwsError::validation(
            "One or more parameter values were invalid: \
             Value must not be supplied when Exists is false.",
        )),
        (Some(true), None) => Err(AwsError::validation(
            "One or more parameter values were invalid: \
             Value must be supplied when Exists is true.",
        )),
        (None, None) => Err(AwsError::validation(
            "One or more parameter values were invalid: \
             an Expected entry must specify Value, Exists, or ComparisonOperator.",
        )),
        // Exists true (or omitted) with a Value: the attribute must equal it.
        (_, Some(v)) => Ok(format!("{name} = {}", builder.value(v))),
    }
}

fn translate_attribute_updates(
    obj: &mut Map<String, Value>,
    builder: &mut ExprBuilder,
) -> Result<(), AwsError> {
    let Some(updates) = obj.remove("AttributeUpdates") else {
        return Ok(());
    };
    if obj.contains_key("UpdateExpression") {
        return Err(mutually_exclusive("AttributeUpdates", "UpdateExpression"));
    }
    let map = updates
        .as_object()
        .ok_or_else(|| AwsError::validation("AttributeUpdates must be a map."))?;

    let mut sets = Vec::new();
    let mut removes = Vec::new();
    let mut adds = Vec::new();
    let mut deletes = Vec::new();

    for (attr, spec) in map {
        let name = builder.name(attr);
        let spec = spec
            .as_object()
            .ok_or_else(|| AwsError::validation("Each AttributeUpdates entry must be a map."))?;
        let action = spec.get("Action").and_then(Value::as_str).unwrap_or("PUT");
        match action {
            "PUT" => {
                let v = spec.get("Value").ok_or_else(|| {
                    AwsError::validation("AttributeUpdates PUT action requires a Value.")
                })?;
                sets.push(format!("{name} = {}", builder.value(v)));
            }
            "ADD" => {
                let v = spec.get("Value").ok_or_else(|| {
                    AwsError::validation("AttributeUpdates ADD action requires a Value.")
                })?;
                adds.push(format!("{name} {}", builder.value(v)));
            }
            "DELETE" => match spec.get("Value") {
                Some(v) => deletes.push(format!("{name} {}", builder.value(v))),
                None => removes.push(name.clone()),
            },
            other => {
                return Err(AwsError::validation(format!(
                    "Unknown AttributeUpdates Action: {other}; expected PUT, ADD, or DELETE."
                )));
            }
        }
    }

    let mut clauses = Vec::new();
    if !sets.is_empty() {
        clauses.push(format!("SET {}", sets.join(", ")));
    }
    if !removes.is_empty() {
        clauses.push(format!("REMOVE {}", removes.join(", ")));
    }
    if !adds.is_empty() {
        clauses.push(format!("ADD {}", adds.join(", ")));
    }
    if !deletes.is_empty() {
        clauses.push(format!("DELETE {}", deletes.join(", ")));
    }
    if clauses.is_empty() {
        return Ok(());
    }
    obj.insert(
        "UpdateExpression".to_string(),
        Value::String(clauses.join(" ")),
    );
    Ok(())
}

fn translate_key_conditions(
    obj: &mut Map<String, Value>,
    builder: &mut ExprBuilder,
) -> Result<(), AwsError> {
    let Some(conditions) = obj.remove("KeyConditions") else {
        return Ok(());
    };
    if obj.contains_key("KeyConditionExpression") {
        return Err(mutually_exclusive(
            "KeyConditions",
            "KeyConditionExpression",
        ));
    }
    let map = conditions
        .as_object()
        .ok_or_else(|| AwsError::validation("KeyConditions must be a map."))?;

    let mut fragments = Vec::with_capacity(map.len());
    for (attr, cond) in map {
        fragments.push(filter_fragment(builder, attr, cond)?);
    }
    if fragments.is_empty() {
        return Ok(());
    }
    // Key conditions are always AND-joined; ConditionalOperator does not apply.
    obj.insert(
        "KeyConditionExpression".to_string(),
        Value::String(fragments.join(" AND ")),
    );
    Ok(())
}

fn translate_filter(
    obj: &mut Map<String, Value>,
    legacy_key: &str,
    builder: &mut ExprBuilder,
) -> Result<(), AwsError> {
    let Some(filter) = obj.remove(legacy_key) else {
        return Ok(());
    };
    if obj.contains_key("FilterExpression") {
        return Err(mutually_exclusive(legacy_key, "FilterExpression"));
    }
    let conj = conditional_operator(obj);
    let map = filter
        .as_object()
        .ok_or_else(|| AwsError::validation(format!("{legacy_key} must be a map.")))?;

    let mut fragments = Vec::with_capacity(map.len());
    for (attr, cond) in map {
        fragments.push(filter_fragment(builder, attr, cond)?);
    }
    if fragments.is_empty() {
        return Ok(());
    }
    obj.insert(
        "FilterExpression".to_string(),
        Value::String(fragments.join(&format!(" {conj} "))),
    );
    Ok(())
}

/// Translate one `{ComparisonOperator, AttributeValueList}` condition, used by
/// `KeyConditions`, `QueryFilter`, and `ScanFilter`.
fn filter_fragment(
    builder: &mut ExprBuilder,
    attr: &str,
    cond: &Value,
) -> Result<String, AwsError> {
    let cond = cond
        .as_object()
        .ok_or_else(|| AwsError::validation("Each condition must be a map."))?;
    let name = builder.name(attr);
    let op = cond
        .get("ComparisonOperator")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::validation("Each condition requires a ComparisonOperator."))?;
    let list = cond
        .get("AttributeValueList")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    comparison_fragment(builder, &name, op, &list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Rewrite and return an owned result, sidestepping the input lifetime
    /// that the `Cow` carries.
    fn rw(input: Value) -> Value {
        rewrite(&input).unwrap().into_owned()
    }

    fn rw_err(input: Value) -> AwsError {
        rewrite(&input).unwrap_err()
    }

    #[test]
    fn no_legacy_params_borrows_unchanged() {
        let input = json!({"TableName": "t", "ConditionExpression": "a = :b"});
        let out = rewrite(&input).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn expected_value_short_form_becomes_equality() {
        let out = rw(json!({
            "TableName": "t",
            "Expected": { "status": { "Value": {"S": "active"} } },
        }));
        let o = out.as_object().unwrap();
        assert!(o.get("Expected").is_none(), "legacy key must be removed");
        assert_eq!(o["ConditionExpression"], json!("#_le_n0 = :_le_v0"));
        assert_eq!(o["ExpressionAttributeNames"]["#_le_n0"], json!("status"));
        assert_eq!(
            o["ExpressionAttributeValues"][":_le_v0"],
            json!({"S": "active"})
        );
    }

    #[test]
    fn expected_exists_false_becomes_attribute_not_exists() {
        let out = rw(json!({ "Expected": { "pk": { "Exists": false } } }));
        assert_eq!(
            out["ConditionExpression"],
            json!("attribute_not_exists(#_le_n0)")
        );
    }

    #[test]
    fn expected_exists_false_with_value_is_rejected() {
        let err = rw_err(json!({
            "Expected": { "pk": { "Exists": false, "Value": {"S": "x"} } }
        }));
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn expected_comparison_operator_between() {
        let out = rw(json!({
            "Expected": {
                "age": {
                    "ComparisonOperator": "BETWEEN",
                    "AttributeValueList": [{"N": "1"}, {"N": "9"}],
                }
            }
        }));
        assert_eq!(
            out["ConditionExpression"],
            json!("#_le_n0 BETWEEN :_le_v0 AND :_le_v1")
        );
    }

    #[test]
    fn expected_conflicts_with_condition_expression() {
        let err = rw_err(json!({
            "ConditionExpression": "a = :b",
            "Expected": { "x": { "Exists": false } },
        }));
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn attribute_updates_translate_each_action() {
        // One action per call so placeholder numbering stays deterministic.
        assert_eq!(
            rw(json!({"AttributeUpdates": {"a": {"Action": "PUT", "Value": {"S": "x"}}}}))["UpdateExpression"],
            json!("SET #_le_n0 = :_le_v0")
        );
        assert_eq!(
            rw(json!({"AttributeUpdates": {"a": {"Value": {"S": "x"}}}}))["UpdateExpression"],
            json!("SET #_le_n0 = :_le_v0")
        );
        assert_eq!(
            rw(json!({"AttributeUpdates": {"n": {"Action": "ADD", "Value": {"N": "1"}}}}))["UpdateExpression"],
            json!("ADD #_le_n0 :_le_v0")
        );
        assert_eq!(
            rw(json!({"AttributeUpdates": {"s": {"Action": "DELETE", "Value": {"SS": ["x"]}}}}))["UpdateExpression"],
            json!("DELETE #_le_n0 :_le_v0")
        );
        assert_eq!(
            rw(json!({"AttributeUpdates": {"a": {"Action": "DELETE"}}}))["UpdateExpression"],
            json!("REMOVE #_le_n0")
        );
    }

    #[test]
    fn attribute_updates_conflicts_with_update_expression() {
        let err = rw_err(json!({
            "UpdateExpression": "SET a = :b",
            "AttributeUpdates": {"a": {"Value": {"S": "x"}}},
        }));
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn key_conditions_become_key_condition_expression() {
        let out = rw(json!({
            "KeyConditions": {
                "pk": { "ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}] }
            }
        }));
        assert_eq!(out["KeyConditionExpression"], json!("#_le_n0 = :_le_v0"));
    }

    #[test]
    fn query_filter_joins_with_conditional_operator() {
        let out = rw(json!({
            "ConditionalOperator": "OR",
            "QueryFilter": {
                "a": { "ComparisonOperator": "EQ", "AttributeValueList": [{"S": "x"}] },
                "b": { "ComparisonOperator": "EQ", "AttributeValueList": [{"S": "y"}] }
            }
        }));
        let fe = out["FilterExpression"].as_str().unwrap();
        assert!(fe.contains(" OR "), "expected OR join, got {fe}");
    }

    #[test]
    fn scan_filter_becomes_filter_expression() {
        let out = rw(json!({
            "ScanFilter": { "a": { "ComparisonOperator": "NOT_NULL" } }
        }));
        assert_eq!(out["FilterExpression"], json!("attribute_exists(#_le_n0)"));
    }
}
