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

pub fn list_aliases(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let function_name = require_str(input, "FunctionName")?;

    let f = state
        .functions
        .get(function_name)
        .ok_or_else(|| resource_not_found("function", function_name))?;

    let aliases: Vec<Value> = f.aliases.values().map(alias_to_value).collect();

    Ok(json!({ "Aliases": aliases }))
}
