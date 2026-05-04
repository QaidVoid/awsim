use awsim_core::{RequestContext, ServiceHandler};
use awsim_iam::IamService;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("iam", "us-east-1")
}

async fn call(svc: &IamService, op: &str, input: Value) -> Result<Value, awsim_core::AwsError> {
    svc.handle(op, input, &ctx()).await
}

fn valid_policy_doc() -> String {
    json!({
        "Version": "2012-10-17",
        "Statement": [{ "Effect": "Allow", "Action": "*", "Resource": "*" }]
    })
    .to_string()
}

async fn create_policies(svc: &IamService, n: usize) -> Vec<String> {
    let mut arns = Vec::new();
    for i in 0..n {
        let resp = call(
            svc,
            "CreatePolicy",
            json!({
                "PolicyName": format!("p{i}"),
                "PolicyDocument": valid_policy_doc(),
            }),
        )
        .await
        .unwrap();
        arns.push(resp["Policy"]["Arn"].as_str().unwrap().to_string());
    }
    arns
}

#[tokio::test]
async fn attach_user_policy_rejects_eleventh_attachment() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();

    let arns = create_policies(&svc, 11).await;
    for arn in &arns[..10] {
        call(
            &svc,
            "AttachUserPolicy",
            json!({ "UserName": "alice", "PolicyArn": arn }),
        )
        .await
        .unwrap();
    }
    let err = call(
        &svc,
        "AttachUserPolicy",
        json!({ "UserName": "alice", "PolicyArn": &arns[10] }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "LimitExceeded");
}

#[tokio::test]
async fn create_access_key_rejects_third_key() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();

    for _ in 0..2 {
        call(&svc, "CreateAccessKey", json!({ "UserName": "alice" }))
            .await
            .unwrap();
    }
    let err = call(&svc, "CreateAccessKey", json!({ "UserName": "alice" }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "LimitExceeded");
}

#[tokio::test]
async fn add_user_to_group_rejects_eleventh_membership() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();
    for i in 0..11 {
        call(&svc, "CreateGroup", json!({ "GroupName": format!("g{i}") }))
            .await
            .unwrap();
    }
    for i in 0..10 {
        call(
            &svc,
            "AddUserToGroup",
            json!({ "GroupName": format!("g{i}"), "UserName": "alice" }),
        )
        .await
        .unwrap();
    }
    let err = call(
        &svc,
        "AddUserToGroup",
        json!({ "GroupName": "g10", "UserName": "alice" }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "LimitExceeded");
}

#[tokio::test]
async fn add_user_to_group_idempotent_at_cap() {
    // Re-issuing AddUserToGroup for a group the user is already in must
    // succeed even when at the cap, since no new edge is being created.
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();
    for i in 0..10 {
        call(&svc, "CreateGroup", json!({ "GroupName": format!("g{i}") }))
            .await
            .unwrap();
        call(
            &svc,
            "AddUserToGroup",
            json!({ "GroupName": format!("g{i}"), "UserName": "alice" }),
        )
        .await
        .unwrap();
    }
    // Re-add an existing membership — must still succeed.
    call(
        &svc,
        "AddUserToGroup",
        json!({ "GroupName": "g0", "UserName": "alice" }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn create_role_rejects_max_session_duration_below_3600() {
    let svc = IamService::new();
    let err = call(
        &svc,
        "CreateRole",
        json!({
            "RoleName": "r",
            "AssumeRolePolicyDocument": valid_policy_doc(),
            "MaxSessionDuration": 1800u64,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "ValidationError");
}

#[tokio::test]
async fn create_role_rejects_max_session_duration_above_43200() {
    let svc = IamService::new();
    let err = call(
        &svc,
        "CreateRole",
        json!({
            "RoleName": "r",
            "AssumeRolePolicyDocument": valid_policy_doc(),
            "MaxSessionDuration": 50_000u64,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "ValidationError");
}

#[tokio::test]
async fn list_users_paginates_with_marker() {
    let svc = IamService::new();
    for i in 0..5 {
        call(
            &svc,
            "CreateUser",
            json!({ "UserName": format!("u-{i:02}") }),
        )
        .await
        .unwrap();
    }

    let page1 = call(&svc, "ListUsers", json!({ "MaxItems": 2u64 }))
        .await
        .unwrap();
    assert_eq!(page1["IsTruncated"], json!(true));
    let marker = page1["Marker"].as_str().expect("marker on truncated page");
    let names1: Vec<String> = page1["Users"]["member"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["UserName"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names1, vec!["u-00".to_string(), "u-01".to_string()]);

    let page2 = call(
        &svc,
        "ListUsers",
        json!({ "MaxItems": 100u64, "Marker": marker }),
    )
    .await
    .unwrap();
    let names2: Vec<String> = page2["Users"]["member"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["UserName"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names2,
        vec!["u-02".to_string(), "u-03".to_string(), "u-04".to_string()]
    );
    assert_eq!(page2["IsTruncated"], json!(false));
    assert!(page2.get("Marker").is_none());
}

#[tokio::test]
async fn get_user_without_username_returns_caller_when_access_key_matches_user() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();
    call(&svc, "CreateUser", json!({ "UserName": "bob" }))
        .await
        .unwrap();

    // Sign as alice — no UserName supplied. Must resolve to alice, not
    // whichever user iterates first out of the dashmap.
    let mut self_ctx = ctx();
    self_ctx.access_key = Some("alice".to_string());
    let resp = svc.handle("GetUser", json!({}), &self_ctx).await.unwrap();
    assert_eq!(resp["User"]["UserName"], json!("alice"));
}

#[tokio::test]
async fn create_and_list_access_keys_default_to_caller() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();

    let mut self_ctx = ctx();
    self_ctx.access_key = Some("alice".to_string());
    let resp = svc
        .handle("CreateAccessKey", json!({}), &self_ctx)
        .await
        .unwrap();
    assert_eq!(resp["AccessKey"]["UserName"], json!("alice"));

    let listed = svc
        .handle("ListAccessKeys", json!({}), &self_ctx)
        .await
        .unwrap();
    let keys = listed["AccessKeyMetadata"]["member"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["UserName"], json!("alice"));
}

#[tokio::test]
async fn attach_user_policy_accepts_aws_managed_arn() {
    let svc = IamService::new();
    call(&svc, "CreateUser", json!({ "UserName": "alice" }))
        .await
        .unwrap();

    // No policy of this name exists in awsim's local store. AWS-managed
    // ARNs (account literal "aws") should still be accepted.
    call(
        &svc,
        "AttachUserPolicy",
        json!({
            "UserName": "alice",
            "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess",
        }),
    )
    .await
    .unwrap();

    // GetUserPolicies / ListAttachedUserPolicies should now show it.
    let attached = call(
        &svc,
        "ListAttachedUserPolicies",
        json!({ "UserName": "alice" }),
    )
    .await
    .unwrap();
    let arns: Vec<&str> = attached["AttachedPolicies"]["member"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["PolicyArn"].as_str())
        .collect();
    assert!(
        arns.contains(&"arn:aws:iam::aws:policy/AdministratorAccess"),
        "managed ARN missing from attached: {arns:?}"
    );
}

#[tokio::test]
async fn attach_role_policy_accepts_service_role_managed_path() {
    // arn:aws:iam::aws:policy/service-role/<name> (path-prefixed managed
    // ARN) is also valid — used by Lambda execution roles, ECS tasks,
    // and many CDK/CFN templates.
    let svc = IamService::new();
    call(
        &svc,
        "CreateRole",
        json!({
            "RoleName": "lambda-exec",
            "AssumeRolePolicyDocument": valid_policy_doc(),
        }),
    )
    .await
    .unwrap();
    call(
        &svc,
        "AttachRolePolicy",
        json!({
            "RoleName": "lambda-exec",
            "PolicyArn": "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole",
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn delete_group_blocks_when_inline_policy_present() {
    let svc = IamService::new();
    call(&svc, "CreateGroup", json!({ "GroupName": "g" }))
        .await
        .unwrap();
    call(
        &svc,
        "PutGroupPolicy",
        json!({
            "GroupName": "g",
            "PolicyName": "inline-1",
            "PolicyDocument": valid_policy_doc(),
        }),
    )
    .await
    .unwrap();

    let err = call(&svc, "DeleteGroup", json!({ "GroupName": "g" }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "DeleteConflict");
    assert!(err.message.contains("inline"));
}

#[tokio::test]
async fn create_role_accepts_max_session_duration_at_boundary() {
    let svc = IamService::new();
    call(
        &svc,
        "CreateRole",
        json!({
            "RoleName": "r1",
            "AssumeRolePolicyDocument": valid_policy_doc(),
            "MaxSessionDuration": 3600u64,
        }),
    )
    .await
    .unwrap();
    call(
        &svc,
        "CreateRole",
        json!({
            "RoleName": "r2",
            "AssumeRolePolicyDocument": valid_policy_doc(),
            "MaxSessionDuration": 43_200u64,
        }),
    )
    .await
    .unwrap();
}
