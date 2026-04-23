use awsim_core::{RequestContext, ServiceHandler};
use awsim_iam::IamService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("iam", "us-east-1")
}

async fn call(svc: &IamService, op: &str, input: Value) -> Result<Value, awsim_core::AwsError> {
    svc.handle(op, input, &ctx()).await
}

fn valid_policy() -> Value {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": "s3:GetObject",
            "Resource": "*"
        }]
    })
}

#[tokio::test]
async fn create_policy_rejects_invalid_effect() {
    let svc = IamService::new();
    let bad = json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Permit",
            "Action": "s3:GetObject",
            "Resource": "*"
        }]
    });
    let err = call(
        &svc,
        "CreatePolicy",
        json!({
            "PolicyName": "BadPolicy",
            "PolicyDocument": bad.to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "MalformedPolicyDocument");
    assert!(err.message.to_lowercase().contains("permit"), "msg: {}", err.message);
}

#[tokio::test]
async fn create_policy_rejects_missing_statement() {
    let svc = IamService::new();
    let bad = json!({ "Version": "2012-10-17" });
    let err = call(
        &svc,
        "CreatePolicy",
        json!({
            "PolicyName": "NoStmt",
            "PolicyDocument": bad.to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "MalformedPolicyDocument");
    assert!(err.message.contains("Statement"), "msg: {}", err.message);
}

#[tokio::test]
async fn create_policy_accepts_valid_document() {
    let svc = IamService::new();
    let doc = valid_policy().to_string();
    let result = call(
        &svc,
        "CreatePolicy",
        json!({
            "PolicyName": "GoodPolicy",
            "PolicyDocument": doc,
        }),
    )
    .await
    .unwrap();
    assert!(result.get("Policy").is_some());
}

#[tokio::test]
async fn put_user_policy_rejects_invalid() {
    let svc = IamService::new();
    call(
        &svc,
        "CreateUser",
        json!({ "UserName": "alice" }),
    )
    .await
    .unwrap();
    let bad = json!({ "Version": "2012-10-17", "Statement": [{ "Effect": "Nope", "Action": "*", "Resource": "*" }] });
    let err = call(
        &svc,
        "PutUserPolicy",
        json!({
            "UserName": "alice",
            "PolicyName": "inline",
            "PolicyDocument": bad.to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "MalformedPolicyDocument");
}

#[tokio::test]
async fn simulate_custom_policy_allow() {
    let svc = IamService::new();
    let doc = valid_policy().to_string();
    let result = call(
        &svc,
        "SimulateCustomPolicy",
        json!({
            "PolicyInputList": [doc],
            "ActionNames": ["s3:GetObject"],
            "ResourceArns": ["arn:aws:s3:::bucket/key"],
        }),
    )
    .await
    .unwrap();
    let results = result["EvaluationResults"]["member"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["EvalDecision"], "allowed");
    assert_eq!(results[0]["EvalActionName"], "s3:GetObject");
}

#[tokio::test]
async fn simulate_custom_policy_explicit_deny() {
    let svc = IamService::new();
    let doc = json!({
        "Version": "2012-10-17",
        "Statement": [
            {"Effect": "Allow", "Action": "s3:*", "Resource": "*"},
            {"Effect": "Deny", "Action": "s3:DeleteObject", "Resource": "*"}
        ]
    })
    .to_string();
    let result = call(
        &svc,
        "SimulateCustomPolicy",
        json!({
            "PolicyInputList": [doc],
            "ActionNames": ["s3:DeleteObject"],
            "ResourceArns": ["arn:aws:s3:::bucket/key"],
        }),
    )
    .await
    .unwrap();
    let results = result["EvaluationResults"]["member"].as_array().unwrap();
    assert_eq!(results[0]["EvalDecision"], "explicitDeny");
}

#[tokio::test]
async fn simulate_custom_policy_implicit_deny() {
    let svc = IamService::new();
    let doc = json!({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*"}]
    })
    .to_string();
    let result = call(
        &svc,
        "SimulateCustomPolicy",
        json!({
            "PolicyInputList": [doc],
            "ActionNames": ["ec2:TerminateInstances"],
            "ResourceArns": ["*"],
        }),
    )
    .await
    .unwrap();
    let results = result["EvaluationResults"]["member"].as_array().unwrap();
    assert_eq!(results[0]["EvalDecision"], "implicitDeny");
}

#[tokio::test]
async fn simulate_principal_policy_with_attached_managed_policy() {
    let svc = IamService::new();
    let ctx = RequestContext::new("iam", "us-east-1");

    svc.handle(
        "CreateUser",
        json!({ "UserName": "bob" }),
        &ctx,
    )
    .await
    .unwrap();

    let policy_doc = valid_policy().to_string();
    let created = svc
        .handle(
            "CreatePolicy",
            json!({
                "PolicyName": "BobRead",
                "PolicyDocument": policy_doc,
            }),
            &ctx,
        )
        .await
        .unwrap();
    let policy_arn = created["Policy"]["Arn"].as_str().unwrap().to_string();

    svc.handle(
        "AttachUserPolicy",
        json!({
            "UserName": "bob",
            "PolicyArn": policy_arn,
        }),
        &ctx,
    )
    .await
    .unwrap();

    let user = svc
        .handle("GetUser", json!({ "UserName": "bob" }), &ctx)
        .await
        .unwrap();
    let user_arn = user["User"]["Arn"].as_str().unwrap().to_string();

    let result = svc
        .handle(
            "SimulatePrincipalPolicy",
            json!({
                "PolicySourceArn": user_arn,
                "ActionNames": ["s3:GetObject", "ec2:RunInstances"],
                "ResourceArns": ["arn:aws:s3:::bucket/key"],
            }),
            &ctx,
        )
        .await
        .unwrap();

    let members = result["EvaluationResults"]["member"].as_array().unwrap();
    let get_obj = members
        .iter()
        .find(|r| r["EvalActionName"] == "s3:GetObject")
        .unwrap();
    assert_eq!(get_obj["EvalDecision"], "allowed");
    let run_inst = members
        .iter()
        .find(|r| r["EvalActionName"] == "ec2:RunInstances")
        .unwrap();
    assert_eq!(run_inst["EvalDecision"], "implicitDeny");
}
