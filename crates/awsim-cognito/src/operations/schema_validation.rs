//! Cognito schema-attribute validation.
//!
//! Real Cognito refuses to set, update, or delete an attribute that
//! is not declared in the user pool's schema. The validators here
//! enforce that contract on every write path
//! (`SignUp` / `AdminCreateUser` / `UpdateUserAttributes` /
//! `AdminUpdateUserAttributes` / `AdminDeleteUserAttributes`).
//!
//! Errors are batched into a single `InvalidParameterException` with
//! a semicolon-joined message that matches AWS's wire shape closely
//! enough for retry / diagnostic logic to recognise it.

use std::collections::HashMap;

use awsim_core::AwsError;

use crate::state::SchemaAttribute;

/// Validate a set of attribute names + values against a pool's schema.
///
/// Checks, per attribute:
/// 1. The name exists in the schema (so undeclared `custom:` attrs
///    are rejected with the AWS-style "Attribute does not exist in
///    the schema" message).
/// 2. The value parses for the declared `AttributeDataType`
///    (`Number` -> any decimal, `Boolean` -> exactly "true" / "false",
///    `DateTime` -> non-empty `digit-and-dash` shape, `String` ->
///    accepted as-is).
/// 3. `StringAttributeConstraints` (`MinLength` / `MaxLength`).
/// 4. `NumberAttributeConstraints` (`MinValue` / `MaxValue`).
///
/// All violations are collected and returned in one error to match
/// AWS's batched style.
pub fn validate_attribute_values(
    schema: &[SchemaAttribute],
    attrs: &HashMap<String, String>,
) -> Result<(), AwsError> {
    let mut problems: Vec<String> = Vec::new();

    for (name, value) in attrs {
        match find_schema_attr(schema, name) {
            None => problems.push(format!("{name}: Attribute does not exist in the schema.")),
            Some(attr) => {
                if let Err(msg) = validate_one_value(attr, value) {
                    problems.push(format!("{name}: {msg}"));
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Attributes did not conform to the schema: {}",
                problems.join("; ")
            ),
        ))
    }
}

/// Verify every `Required: true` attr in the schema is present in
/// `attrs`. Used at user-creation time only - real Cognito only
/// checks Required at SignUp / AdminCreateUser, never on update.
///
/// `sub` is excluded because awsim auto-generates it inside
/// `make_user`; it is never user-supplied.
pub fn validate_required_present(
    schema: &[SchemaAttribute],
    attrs: &HashMap<String, String>,
) -> Result<(), AwsError> {
    let mut missing: Vec<String> = Vec::new();
    for attr in schema {
        if !attr.required || attr.name == "sub" {
            continue;
        }
        if !attrs.contains_key(&attr.name) {
            missing.push(attr.name.clone());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Attributes did not conform to the schema: required attribute missing: {}",
                missing.join(", ")
            ),
        ))
    }
}

/// Reject updates that change an attribute whose schema entry has
/// `Mutable: false` to a different value. Attrs not yet set on the
/// user are allowed (mutable=false means "set once and freeze",
/// matching real Cognito).
pub fn validate_mutability(
    schema: &[SchemaAttribute],
    existing: &HashMap<String, String>,
    new_attrs: &HashMap<String, String>,
) -> Result<(), AwsError> {
    let mut violations: Vec<String> = Vec::new();
    for (name, new_value) in new_attrs {
        let Some(attr) = find_schema_attr(schema, name) else {
            continue;
        };
        if attr.mutable {
            continue;
        }
        if let Some(prev) = existing.get(name)
            && prev != new_value
        {
            violations.push(name.clone());
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "user.{}: Attribute cannot be updated.",
                violations.join(", user.")
            ),
        ))
    }
}

/// For `AdminDeleteUserAttributes`: the names being deleted must
/// exist in the schema and be mutable. Required attrs can still be
/// deleted - real Cognito allows that.
pub fn validate_deletable_names(
    schema: &[SchemaAttribute],
    names: &[String],
) -> Result<(), AwsError> {
    let mut problems: Vec<String> = Vec::new();
    for name in names {
        match find_schema_attr(schema, name) {
            None => problems.push(format!("{name}: Attribute does not exist in the schema.")),
            Some(attr) if !attr.mutable => {
                problems.push(format!("{name}: attribute is not mutable."));
            }
            _ => {}
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Attributes did not conform to the schema: {}",
                problems.join("; ")
            ),
        ))
    }
}

fn find_schema_attr<'a>(schema: &'a [SchemaAttribute], name: &str) -> Option<&'a SchemaAttribute> {
    schema.iter().find(|a| a.name == name)
}

fn validate_one_value(attr: &SchemaAttribute, value: &str) -> Result<(), String> {
    match attr.attribute_data_type.as_str() {
        "String" => {
            if let Some(c) = &attr.string_attribute_constraints {
                let len = value.chars().count() as u32;
                if let Some(min) = c.min_length
                    && len < min
                {
                    return Err(format!("value shorter than min length {min}"));
                }
                if let Some(max) = c.max_length
                    && len > max
                {
                    return Err(format!("value longer than max length {max}"));
                }
            }
            Ok(())
        }
        "Number" => {
            let parsed: f64 = value
                .parse()
                .map_err(|_| format!("expected Number, got non-numeric value: {value:?}"))?;
            if let Some(c) = &attr.number_attribute_constraints {
                if let Some(min) = c.min_value
                    && parsed < min as f64
                {
                    return Err(format!("value less than min {min}"));
                }
                if let Some(max) = c.max_value
                    && parsed > max as f64
                {
                    return Err(format!("value greater than max {max}"));
                }
            }
            Ok(())
        }
        "Boolean" => {
            if value == "true" || value == "false" {
                Ok(())
            } else {
                Err(format!(
                    "expected Boolean ('true' or 'false'), got {value:?}"
                ))
            }
        }
        "DateTime" => {
            // Real Cognito accepts a wide range of date strings here
            // (epoch millis as a string, or ISO-8601). Reject only
            // the obviously-malformed: empty, or no digits at all.
            if value.is_empty() || !value.chars().any(|c| c.is_ascii_digit()) {
                Err(format!("expected DateTime, got {value:?}"))
            } else {
                Ok(())
            }
        }
        other => Err(format!("unknown AttributeDataType {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_with(attrs: Vec<SchemaAttribute>) -> Vec<SchemaAttribute> {
        attrs
    }

    fn s(name: &str, required: bool, mutable: bool) -> SchemaAttribute {
        SchemaAttribute {
            name: name.to_string(),
            attribute_data_type: "String".to_string(),
            required,
            mutable,
            developer_only_attribute: false,
            string_attribute_constraints: None,
            number_attribute_constraints: None,
        }
    }

    fn n(name: &str, min: Option<i64>, max: Option<i64>) -> SchemaAttribute {
        SchemaAttribute {
            name: name.to_string(),
            attribute_data_type: "Number".to_string(),
            required: false,
            mutable: true,
            developer_only_attribute: false,
            string_attribute_constraints: None,
            number_attribute_constraints: Some(crate::state::NumberAttributeConstraints {
                min_value: min,
                max_value: max,
            }),
        }
    }

    #[test]
    fn unknown_attr_is_rejected() {
        let schema = schema_with(vec![s("email", false, true)]);
        let mut attrs = HashMap::new();
        attrs.insert("custom:plan".to_string(), "enterprise".to_string());
        let err = validate_attribute_values(&schema, &attrs).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("custom:plan"));
        assert!(err.message.contains("does not exist in the schema"));
    }

    #[test]
    fn declared_attr_passes() {
        let schema = schema_with(vec![s("custom:plan", false, true)]);
        let mut attrs = HashMap::new();
        attrs.insert("custom:plan".to_string(), "enterprise".to_string());
        validate_attribute_values(&schema, &attrs).unwrap();
    }

    #[test]
    fn number_type_rejects_non_numeric() {
        let schema = schema_with(vec![n("custom:rank", None, None)]);
        let mut attrs = HashMap::new();
        attrs.insert("custom:rank".to_string(), "high".to_string());
        let err = validate_attribute_values(&schema, &attrs).unwrap_err();
        assert!(err.message.contains("non-numeric"));
    }

    #[test]
    fn number_constraints_enforced() {
        let schema = schema_with(vec![n("custom:rank", Some(1), Some(10))]);
        let mut ok = HashMap::new();
        ok.insert("custom:rank".to_string(), "5".to_string());
        validate_attribute_values(&schema, &ok).unwrap();

        let mut too_low = HashMap::new();
        too_low.insert("custom:rank".to_string(), "0".to_string());
        let err = validate_attribute_values(&schema, &too_low).unwrap_err();
        assert!(err.message.contains("less than min"));

        let mut too_high = HashMap::new();
        too_high.insert("custom:rank".to_string(), "11".to_string());
        let err = validate_attribute_values(&schema, &too_high).unwrap_err();
        assert!(err.message.contains("greater than max"));
    }

    #[test]
    fn boolean_must_be_exact_strings() {
        let schema = schema_with(vec![SchemaAttribute {
            name: "custom:flag".to_string(),
            attribute_data_type: "Boolean".to_string(),
            required: false,
            mutable: true,
            developer_only_attribute: false,
            string_attribute_constraints: None,
            number_attribute_constraints: None,
        }]);
        let mut bad = HashMap::new();
        bad.insert("custom:flag".to_string(), "yes".to_string());
        let err = validate_attribute_values(&schema, &bad).unwrap_err();
        assert!(err.message.contains("Boolean"));

        let mut good = HashMap::new();
        good.insert("custom:flag".to_string(), "true".to_string());
        validate_attribute_values(&schema, &good).unwrap();
    }

    #[test]
    fn string_min_max_length_enforced() {
        let schema = schema_with(vec![SchemaAttribute {
            name: "custom:tag".to_string(),
            attribute_data_type: "String".to_string(),
            required: false,
            mutable: true,
            developer_only_attribute: false,
            string_attribute_constraints: Some(crate::state::StringAttributeConstraints {
                min_length: Some(2),
                max_length: Some(5),
            }),
            number_attribute_constraints: None,
        }]);
        let mut too_short = HashMap::new();
        too_short.insert("custom:tag".to_string(), "x".to_string());
        let err = validate_attribute_values(&schema, &too_short).unwrap_err();
        assert!(err.message.contains("shorter than min"));

        let mut too_long = HashMap::new();
        too_long.insert("custom:tag".to_string(), "abcdef".to_string());
        let err = validate_attribute_values(&schema, &too_long).unwrap_err();
        assert!(err.message.contains("longer than max"));
    }

    #[test]
    fn required_present_check() {
        let schema = schema_with(vec![
            s("sub", true, false), // sub is excluded by the helper
            s("custom:org", true, true),
            s("email", false, true),
        ]);
        let mut just_email = HashMap::new();
        just_email.insert("email".to_string(), "user@example.com".to_string());
        let err = validate_required_present(&schema, &just_email).unwrap_err();
        assert!(err.message.contains("custom:org"));
        assert!(!err.message.contains(" sub"));

        let mut with_org = just_email.clone();
        with_org.insert("custom:org".to_string(), "acme".to_string());
        validate_required_present(&schema, &with_org).unwrap();
    }

    #[test]
    fn mutability_check() {
        let schema = schema_with(vec![s("custom:org", false, false)]);
        let mut existing = HashMap::new();
        existing.insert("custom:org".to_string(), "acme".to_string());

        // Same value -> ok.
        let mut same = HashMap::new();
        same.insert("custom:org".to_string(), "acme".to_string());
        validate_mutability(&schema, &existing, &same).unwrap();

        // Different value -> rejected.
        let mut changed = HashMap::new();
        changed.insert("custom:org".to_string(), "globex".to_string());
        let err = validate_mutability(&schema, &existing, &changed).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");

        // First-time set (no prior value) -> ok.
        let empty = HashMap::new();
        let mut first_set = HashMap::new();
        first_set.insert("custom:org".to_string(), "acme".to_string());
        validate_mutability(&schema, &empty, &first_set).unwrap();
    }

    #[test]
    fn deletable_names_check() {
        let schema = schema_with(vec![s("email", false, true), s("sub", true, false)]);
        validate_deletable_names(&schema, &["email".to_string()]).unwrap();

        let err = validate_deletable_names(&schema, &["custom:missing".to_string()]).unwrap_err();
        assert!(err.message.contains("does not exist"));

        let err = validate_deletable_names(&schema, &["sub".to_string()]).unwrap_err();
        assert!(err.message.contains("not mutable"));
    }
}
