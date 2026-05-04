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
        condition.insert("ArnLike".to_string(), json!({ "AWS:SourceArn": arn }));
    }
    if let Some(acct) = source_account {
        condition.insert(
            "StringEquals".to_string(),
            json!({ "AWS:SourceAccount": acct }),
        );
    }

    // Principal shape mirrors AWS conventions:
    //   * "*"                              → "Principal": "*"          (any caller)
    //   * 12-digit account                 → "Principal": { "AWS": "arn:aws:iam::{acct}:root" }
    //   * arn:aws:iam::*                   → "Principal": { "AWS": "<arn>" }
    //   * everything else (e.g.            → "Principal": { "Service": "<value>" }
    //     "lambda.amazonaws.com")
    // Wrapping every value as {Service:...} as we did before produced
    // policy documents that real IAM evaluation logic rejects for cross-
    // account caller principals.
    let principal_value: Value = if principal == "*" {
        Value::String("*".to_string())
    } else if principal.starts_with("arn:aws:iam::") {
        json!({ "AWS": principal })
    } else if principal.len() == 12 && principal.chars().all(|c| c.is_ascii_digit()) {
        json!({ "AWS": format!("arn:aws:iam::{principal}:root") })
    } else {
        json!({ "Service": principal })
    };

    let statement = json!({
        "Sid": statement_id,
        "Effect": "Allow",
        "Principal": principal_value,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::functions::create_function;
    use crate::state::LambdaState;

    fn ctx() -> RequestContext {
        RequestContext::new("lambda", "us-east-1")
    }

    fn empty_zip_b64() -> String {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        let bytes: [u8; 22] = [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        BASE64.encode(bytes)
    }

    fn create_test_fn(state: &LambdaState) {
        create_function(
            state,
            &json!({
                "FunctionName": "f",
                "Role": "arn:aws:iam::000000000000:role/test",
                "Code": { "ZipFile": empty_zip_b64() },
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn statement_principal(svc_state: &LambdaState, sid: &str) -> Value {
        let f = svc_state.functions.get("f").unwrap();
        let stmt = f.policy_statements.get(sid).unwrap().clone();
        stmt["Principal"].clone()
    }

    #[test]
    fn add_permission_wraps_service_principal_under_service_key() {
        let state = LambdaState::default();
        create_test_fn(&state);
        add_permission(
            &state,
            &json!({
                "FunctionName": "f",
                "StatementId": "s1",
                "Principal": "events.amazonaws.com",
            }),
            &ctx(),
        )
        .unwrap();
        let p = statement_principal(&state, "s1");
        assert_eq!(p["Service"], json!("events.amazonaws.com"));
        assert!(p.get("AWS").is_none());
    }

    #[test]
    fn add_permission_wraps_account_principal_as_aws_root_arn() {
        let state = LambdaState::default();
        create_test_fn(&state);
        add_permission(
            &state,
            &json!({
                "FunctionName": "f",
                "StatementId": "s1",
                "Principal": "111122223333",
            }),
            &ctx(),
        )
        .unwrap();
        let p = statement_principal(&state, "s1");
        assert_eq!(p["AWS"], json!("arn:aws:iam::111122223333:root"));
    }

    #[test]
    fn add_permission_wraps_iam_arn_principal_as_aws() {
        let state = LambdaState::default();
        create_test_fn(&state);
        add_permission(
            &state,
            &json!({
                "FunctionName": "f",
                "StatementId": "s1",
                "Principal": "arn:aws:iam::111122223333:user/alice",
            }),
            &ctx(),
        )
        .unwrap();
        let p = statement_principal(&state, "s1");
        assert_eq!(p["AWS"], json!("arn:aws:iam::111122223333:user/alice"));
    }

    #[test]
    fn add_permission_wildcard_principal_passes_through_as_string() {
        let state = LambdaState::default();
        create_test_fn(&state);
        add_permission(
            &state,
            &json!({
                "FunctionName": "f",
                "StatementId": "s1",
                "Principal": "*",
            }),
            &ctx(),
        )
        .unwrap();
        let p = statement_principal(&state, "s1");
        assert_eq!(p, json!("*"));
    }
}
