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
