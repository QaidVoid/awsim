use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{resource_conflict, resource_not_found},
    state::{Alias, LambdaState},
    util::{opt_str, require_str},
};

fn alias_to_value(alias: &Alias) -> Value {
    json!({
        "Name": alias.name,
        "AliasArn": alias.arn,
        "FunctionVersion": alias.function_version,
        "Description": alias.description,
    })
}

pub fn create_alias(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let function_name = require_str(input, "FunctionName")?;
    let alias_name = require_str(input, "Name")?;
    let function_version = require_str(input, "FunctionVersion")?;
    let description = opt_str(input, "Description").unwrap_or("").to_string();

    let mut f = state
        .functions
        .get_mut(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    if f.aliases.contains_key(alias_name) {
        return Err(resource_conflict(format!(
            "Alias already exists: {alias_name}"
        )));
    }

    let alias_arn = format!("{}:{}", f.arn, alias_name);
    let alias = Alias {
        name: alias_name.to_string(),
        arn: alias_arn,
        function_version: function_version.to_string(),
        description,
    };

    let result = alias_to_value(&alias);
    f.aliases.insert(alias_name.to_string(), alias);

    Ok(result)
}

pub fn get_alias(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let function_name = require_str(input, "FunctionName")?;
    let alias_name = require_str(input, "Name")?;

    let f = state
        .functions
        .get(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    let alias = f
        .aliases
        .get(alias_name)
        .ok_or_else(|| resource_not_found("alias", alias_name))?;

    Ok(alias_to_value(alias))
}

pub fn delete_alias(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let function_name = require_str(input, "FunctionName")?;
    let alias_name = require_str(input, "Name")?;

    let mut f = state
        .functions
        .get_mut(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    f.aliases
        .remove(alias_name)
        .ok_or_else(|| resource_not_found("alias", alias_name))?;

    Ok(json!({}))
}

pub fn update_alias(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let function_name = require_str(input, "FunctionName")?;
    let alias_name = require_str(input, "Name")?;

    let mut f = state
        .functions
        .get_mut(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    let alias = f
        .aliases
        .get_mut(alias_name)
        .ok_or_else(|| resource_not_found("alias", alias_name))?;

    if let Some(version) = opt_str(input, "FunctionVersion") {
        alias.function_version = version.to_string();
    }
    if let Some(description) = opt_str(input, "Description") {
        alias.description = description.to_string();
    }

    Ok(alias_to_value(alias))
}

pub fn list_aliases(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let function_name = require_str(input, "FunctionName")?;

    let f = state
        .functions
        .get(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    let mut all: Vec<Alias> = f.aliases.values().cloned().collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));

    let max = cap_max_results(input.get("MaxItems").and_then(Value::as_i64), 50, 50);
    let marker = input.get("Marker").and_then(Value::as_str);
    let page = paginate(all, max, marker, |a| a.name.clone())?;

    let aliases: Vec<Value> = page.items.iter().map(alias_to_value).collect();
    let mut result = json!({ "Aliases": aliases });
    if let Some(token) = page.next_token {
        result["NextMarker"] = json!(token);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LambdaFunction;
    use std::collections::HashMap;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn state_with_function(name: &str) -> LambdaState {
        let state = LambdaState::default();
        state.functions.insert(
            name.to_string(),
            LambdaFunction {
                name: name.to_string(),
                arn: format!("arn:aws:lambda:us-east-1:000000000000:function:{name}"),
                runtime: Some("nodejs20.x".into()),
                role: "arn:aws:iam::000000000000:role/test".into(),
                handler: Some("index.handler".into()),
                description: String::new(),
                timeout: 3,
                memory_size: 128,
                code_sha256: String::new(),
                code_size: 0,
                code: None,
                environment: HashMap::new(),
                version: "$LATEST".into(),
                versions: vec![],
                aliases: HashMap::new(),
                last_modified: "now".into(),
                state: "Active".into(),
                invocations: vec![],
                policy_statements: HashMap::new(),
                tags: HashMap::new(),
                reserved_concurrent_executions: None,
                provisioned_concurrency: HashMap::new(),
            },
        );
        state
    }

    #[test]
    fn update_alias_changes_version_and_description() {
        let state = state_with_function("f");
        create_alias(
            &state,
            &json!({
                "FunctionName": "f",
                "Name": "live",
                "FunctionVersion": "1",
                "Description": "first",
            }),
            &ctx(),
        )
        .unwrap();

        let updated = update_alias(
            &state,
            &json!({
                "FunctionName": "f",
                "Name": "live",
                "FunctionVersion": "2",
                "Description": "second",
            }),
            &ctx(),
        )
        .unwrap();

        assert_eq!(updated["FunctionVersion"], json!("2"));
        assert_eq!(updated["Description"], json!("second"));

        let got = get_alias(
            &state,
            &json!({ "FunctionName": "f", "Name": "live" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(got["FunctionVersion"], json!("2"));
    }

    #[test]
    fn update_alias_leaves_unspecified_fields_intact() {
        let state = state_with_function("f");
        create_alias(
            &state,
            &json!({
                "FunctionName": "f",
                "Name": "live",
                "FunctionVersion": "1",
                "Description": "keep me",
            }),
            &ctx(),
        )
        .unwrap();

        update_alias(
            &state,
            &json!({
                "FunctionName": "f",
                "Name": "live",
                "FunctionVersion": "2",
            }),
            &ctx(),
        )
        .unwrap();

        let got = get_alias(
            &state,
            &json!({ "FunctionName": "f", "Name": "live" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(got["FunctionVersion"], json!("2"));
        assert_eq!(got["Description"], json!("keep me"));
    }

    #[test]
    fn function_configuration_includes_last_update_status() {
        use crate::operations::functions::function_configuration;
        let state = state_with_function("f");
        let f = state.functions.get("f").unwrap();
        let cfg = function_configuration(&f);
        assert_eq!(cfg["LastUpdateStatus"], json!("Successful"));
        // FunctionArn must appear exactly once with the function's ARN —
        // serde_json silently drops earlier duplicates so this is more of
        // a regression guard against re-adding the duplicated key.
        assert_eq!(cfg["FunctionArn"], json!(f.arn));
    }

    #[test]
    fn update_alias_returns_resource_not_found_for_missing_alias() {
        let state = state_with_function("f");
        let err = update_alias(
            &state,
            &json!({
                "FunctionName": "f",
                "Name": "ghost",
                "FunctionVersion": "1",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }
}
