use awsim_iam_policy::{
    AuthzRequest, ContextValue, Decision, EvalContext, PolicyDocument, evaluate, parse,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

fn req<'a>(
    principal_arn: &'a str,
    principal_account: &'a str,
    action: &'a str,
    resource_arn: &'a str,
    context: &'a HashMap<String, ContextValue>,
) -> AuthzRequest<'a> {
    AuthzRequest {
        principal_arn,
        principal_account,
        action,
        resource_arn,
        context,
    }
}

fn admin() -> PolicyDocument {
    parse(r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#).unwrap()
}

fn s3_read() -> PolicyDocument {
    parse(
        r#"{"Statement":[{"Effect":"Allow","Action":["s3:Get*","s3:List*"],"Resource":["arn:aws:s3:::b","arn:aws:s3:::b/*"]}]}"#,
    )
    .unwrap()
}

#[test]
fn implicit_deny_when_no_policy() {
    let ctx = HashMap::new();
    let r = req("arn:aws:iam::1:user/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &EvalContext::default()), Decision::ImplicitDeny);
}

#[test]
fn allow_admin_wildcard() {
    let ctx = HashMap::new();
    let pol = admin();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:user/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn allow_specific_action_resource() {
    let ctx = HashMap::new();
    let identity = vec![s3_read()];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:user/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn implicit_deny_action_not_in_policy() {
    let ctx = HashMap::new();
    let identity = vec![s3_read()];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:PutObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn explicit_deny_overrides_allow() {
    let ctx = HashMap::new();
    let allow = admin();
    let deny = parse(
        r#"{"Statement":[{"Effect":"Deny","Action":"s3:DeleteObject","Resource":"*"}]}"#,
    )
    .unwrap();
    let identity = vec![allow, deny];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:DeleteObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ExplicitDeny);
}

#[test]
fn not_action_inversion() {
    let ctx = HashMap::new();
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","NotAction":"s3:DeleteObject","Resource":"*"}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let allowed = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:DeleteObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn wildcard_action_pattern() {
    let ctx = HashMap::new();
    let pol = parse(r#"{"Statement":[{"Effect":"Allow","Action":"ec2:Describe*","Resource":"*"}]}"#).unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "ec2:DescribeInstances",
        "arn:aws:ec2:us-east-1:1:instance/i-1",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
    let r2 = req(
        "arn:aws:iam::1:user/x",
        "1",
        "ec2:RunInstances",
        "arn:aws:ec2:us-east-1:1:instance/i-1",
        &ctx,
    );
    assert_eq!(evaluate(&r2, &ec), Decision::ImplicitDeny);
}

#[test]
fn wildcard_resource_pattern() {
    let ctx = HashMap::new();
    let pol = parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::team-*/*"}]}"#).unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::team-alpha/secret",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
    let r2 = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::other/secret",
        &ctx,
    );
    assert_eq!(evaluate(&r2, &ec), Decision::ImplicitDeny);
}

#[test]
fn permissions_boundary_intersection() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let boundary = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
        ..Default::default()
    };
    let allowed = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::1:user/x",
        "1",
        "ec2:RunInstances",
        "arn:aws:ec2:us-east-1:1:instance/i-1",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn session_policy_intersection() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let session = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        session_policy: Some(&session),
        ..Default::default()
    };
    let allowed = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:DeleteObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn scp_intersection_required() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let scp = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#,
    )
    .unwrap();
    let scps = vec![scp];
    let ec = EvalContext {
        identity_policies: &identity,
        scps: &scps,
        ..Default::default()
    };
    let allowed = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::1:u/x",
        "1",
        "ec2:RunInstances",
        "arn:aws:ec2:us-east-1:1:instance/i-1",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn scp_explicit_deny() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let scp = parse(
        r#"{"Statement":[{"Effect":"Deny","Action":"iam:*","Resource":"*"}]}"#,
    )
    .unwrap();
    let scps = vec![scp];
    let ec = EvalContext {
        identity_policies: &identity,
        scps: &scps,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "iam:CreateUser",
        "arn:aws:iam::1:user/y",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ExplicitDeny);
}

#[test]
fn resource_policy_same_account_grants_alone() {
    let ctx = HashMap::new();
    let bucket_policy = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::1:user/alice"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        resource_policy: Some(&bucket_policy),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn resource_policy_cross_account_requires_both() {
    let ctx = HashMap::new();
    let kms_policy = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::222:user/alice"},
            "Action":"kms:Decrypt",
            "Resource":"arn:aws:kms:us-east-1:111:key/abc"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        resource_policy: Some(&kms_policy),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::222:user/alice",
        "222",
        "kms:Decrypt",
        "arn:aws:kms:us-east-1:111:key/abc",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);

    let identity = vec![
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"kms:Decrypt","Resource":"*"}]}"#)
            .unwrap(),
    ];
    let ec2 = EvalContext {
        identity_policies: &identity,
        resource_policy: Some(&kms_policy),
        ..Default::default()
    };
    assert_eq!(evaluate(&r, &ec2), Decision::Allow);
}

#[test]
fn resource_policy_principal_mismatch() {
    let ctx = HashMap::new();
    let bucket_policy = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::1:user/bob"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        resource_policy: Some(&bucket_policy),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_string_equals_match() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:username".to_string(),
        ContextValue::String("alice".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringEquals":{"aws:username":"alice"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_string_equals_mismatch() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:username".to_string(),
        ContextValue::String("bob".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringEquals":{"aws:username":"alice"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_string_like_wildcards() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:userid".to_string(),
        ContextValue::String("AIDAEXAMPLEXYZ".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringLike":{"aws:userid":"AIDA*"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_numeric_less_than() {
    let mut ctx = HashMap::new();
    ctx.insert("s3:max-keys".to_string(), ContextValue::Number(50.0));
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:ListBucket","Resource":"*",
        "Condition":{"NumericLessThan":{"s3:max-keys":"100"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:ListBucket", "arn:aws:s3:::b", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);

    let mut ctx2 = HashMap::new();
    ctx2.insert("s3:max-keys".to_string(), ContextValue::Number(150.0));
    let r2 = req("arn:aws:iam::1:u/x", "1", "s3:ListBucket", "arn:aws:s3:::b", &ctx2);
    assert_eq!(evaluate(&r2, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_ip_address_cidr() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SourceIp".to_string(),
        ContextValue::Ip("10.0.5.7".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"IpAddress":{"aws:SourceIp":"10.0.0.0/8"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);

    let mut ctx2 = HashMap::new();
    ctx2.insert(
        "aws:SourceIp".to_string(),
        ContextValue::Ip("8.8.8.8".into()),
    );
    let r2 = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx2);
    assert_eq!(evaluate(&r2, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_not_ip_address() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SourceIp".to_string(),
        ContextValue::Ip("8.8.8.8".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"NotIpAddress":{"aws:SourceIp":"10.0.0.0/8"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_date_less_than() {
    let mut ctx = HashMap::new();
    let now: DateTime<Utc> = "2024-01-01T00:00:00Z".parse().unwrap();
    ctx.insert("aws:CurrentTime".to_string(), ContextValue::Date(now));
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"DateLessThan":{"aws:CurrentTime":"2025-01-01T00:00:00Z"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_bool_true() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SecureTransport".to_string(),
        ContextValue::Bool(true),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"Bool":{"aws:SecureTransport":"true"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_bool_false_deny() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SecureTransport".to_string(),
        ContextValue::Bool(false),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Deny","Action":"*","Resource":"*",
        "Condition":{"Bool":{"aws:SecureTransport":"false"}}}]}"#,
    )
    .unwrap();
    let identity = vec![admin(), pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ExplicitDeny);
}

#[test]
fn condition_null_absent() {
    let ctx = HashMap::new();
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"Null":{"aws:MultiFactorAuthAge":"true"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_null_present_required() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:MultiFactorAuthAge".to_string(),
        ContextValue::Number(60.0),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"Null":{"aws:MultiFactorAuthAge":"false"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_arn_like() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SourceArn".to_string(),
        ContextValue::String("arn:aws:lambda:us-east-1:1:function:f".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"ArnLike":{"aws:SourceArn":"arn:aws:lambda:*:1:function:*"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_if_exists_absent() {
    let ctx = HashMap::new();
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringEqualsIfExists":{"aws:username":"alice"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_if_exists_present_must_match() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:username".to_string(),
        ContextValue::String("bob".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringEqualsIfExists":{"aws:username":"alice"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_for_any_value_match() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:TagKeys".to_string(),
        ContextValue::StringList(vec!["env".into(), "team".into()]),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"ForAnyValue:StringEquals":{"aws:TagKeys":["team","cost"]}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_for_all_values_satisfied() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:TagKeys".to_string(),
        ContextValue::StringList(vec!["env".into(), "team".into()]),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"ForAllValues:StringEquals":{"aws:TagKeys":["env","team","cost"]}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_for_all_values_violated() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:TagKeys".to_string(),
        ContextValue::StringList(vec!["env".into(), "secret".into()]),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"ForAllValues:StringEquals":{"aws:TagKeys":["env","team"]}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn condition_string_equals_ignore_case() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:username".to_string(),
        ContextValue::String("ALICE".into()),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"StringEqualsIgnoreCase":{"aws:username":"alice"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_numeric_greater_than_equals() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:MultiFactorAuthAge".to_string(),
        ContextValue::Number(3600.0),
    );
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"NumericGreaterThanEquals":{"aws:MultiFactorAuthAge":"1800"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_date_with_epoch() {
    let mut ctx = HashMap::new();
    let dt: DateTime<Utc> = Utc::now();
    ctx.insert("aws:CurrentTime".to_string(), ContextValue::Date(dt));
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
        "Condition":{"DateGreaterThan":{"aws:CurrentTime":"0"}}}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let r = req("arn:aws:iam::1:u/x", "1", "s3:GetObject", "arn:aws:s3:::b/k", &ctx);
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn explicit_deny_in_resource_policy() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let bucket = parse(
        r#"{"Statement":[{
            "Effect":"Deny",
            "Principal":"*",
            "Action":"s3:DeleteObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        resource_policy: Some(&bucket),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:DeleteObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ExplicitDeny);
}

#[test]
fn principal_root_arn_match() {
    let ctx = HashMap::new();
    let bucket = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::1:root"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        resource_policy: Some(&bucket),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}
