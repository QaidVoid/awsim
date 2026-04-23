use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    state::LambdaState,
    util::{opt_str, require_str},
};

// ---------------------------------------------------------------------------
// GetPolicy
// ---------------------------------------------------------------------------

pub fn get_policy(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;

    let func = state
        .functions
        .get(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    if func.policy_statements.is_empty() {
        // No policy — real Lambda returns 404 here; we do too.
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No policy is associated with function: {name}"),
        ));
    }

    let statements: Vec<Value> = func.policy_statements.values().cloned().collect();
    let policy = json!({
        "Version": "2012-10-17",
        "Id": "default",
        "Statement": statements,
    });

    Ok(json!({ "Policy": policy.to_string(), "RevisionId": "1" }))
}

// ---------------------------------------------------------------------------
// AddPermission
// ---------------------------------------------------------------------------

pub fn add_permission(
    state: &LambdaState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let statement_id = require_str(input, "StatementId")?;

    let mut func = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    if func.policy_statements.contains_key(statement_id) {
        return Err(AwsError::conflict(
            "ResourceConflictException",
            format!("Permission already exists with statement id: {statement_id}"),
        ));
    }

    let action = opt_str(input, "Action").unwrap_or("lambda:InvokeFunction");
    let principal = opt_str(input, "Principal").unwrap_or("*");
    let source_arn = input.get("SourceArn").and_then(|v| v.as_str());
    let source_account = opt_str(input, "SourceAccount");

    let function_arn = format!(
        "arn:aws:lambda:{}:{}:function:{}",
        ctx.region, ctx.account_id, name
    );

    let mut condition = serde_json::Map::new();
    if let Some(arn) = source_arn {
        condition.insert(
            "ArnLike".to_string(),
            json!({ "AWS:SourceArn": arn }),
        );
    }
    if let Some(acct) = source_account {
        condition.insert(
            "StringEquals".to_string(),
            json!({ "AWS:SourceAccount": acct }),
        );
    }

    let statement = json!({
        "Sid": statement_id,
        "Effect": "Allow",
        "Principal": { "Service": principal },
        "Action": action,
        "Resource": function_arn,
        "Condition": condition,
    });

    func.policy_statements
        .insert(statement_id.to_string(), statement.clone());

    Ok(json!({ "Statement": statement.to_string() }))
}

// ---------------------------------------------------------------------------
// RemovePermission
// ---------------------------------------------------------------------------

pub fn remove_permission(
    state: &LambdaState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FunctionName")?;
    let statement_id = require_str(input, "StatementId")?;

    let mut func = state
        .functions
        .get_mut(name)
        .ok_or_else(|| resource_not_found("function", name))?;

    func.policy_statements.remove(statement_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No permission with statement id: {statement_id}"),
        )
    })?;

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetAccountSettings
// ---------------------------------------------------------------------------

pub fn get_account_settings(
    _state: &LambdaState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "AccountLimit": {
            "TotalCodeSize": 80530636800i64,
            "CodeSizeUnzipped": 262144000,
            "CodeSizeZipped": 52428800,
            "ConcurrentExecutions": 1000,
            "UnreservedConcurrentExecutions": 1000,
        },
        "AccountUsage": {
            "TotalCodeSize": 0,
            "FunctionCount": 0,
        },
    }))
}
