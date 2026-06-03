use awsim_iam_policy::{
    AuthzRequest, ContextValue, Decision, DecisionReason, EvalContext, PolicyAttribution,
    PolicyAttributions, PolicyDocument, PolicySource, evaluate, evaluate_detailed, explain, parse,
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
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(
        evaluate(&r, &EvalContext::default()),
        Decision::ImplicitDeny
    );
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
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let deny =
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"s3:DeleteObject","Resource":"*"}]}"#)
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
    let pol =
        parse(r#"{"Statement":[{"Effect":"Allow","NotAction":"s3:DeleteObject","Resource":"*"}]}"#)
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
    let pol =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"ec2:Describe*","Resource":"*"}]}"#)
            .unwrap();
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
    let boundary =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#)
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
    let session =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#)
            .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        session_policy: Some(&session),
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
    let scp =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#).unwrap();
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
    let scp =
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"iam:*","Resource":"*"}]}"#).unwrap();
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:ListBucket",
        "arn:aws:s3:::b",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);

    let mut ctx2 = HashMap::new();
    ctx2.insert("s3:max-keys".to_string(), ContextValue::Number(150.0));
    let r2 = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:ListBucket",
        "arn:aws:s3:::b",
        &ctx2,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);

    let mut ctx2 = HashMap::new();
    ctx2.insert(
        "aws:SourceIp".to_string(),
        ContextValue::Ip("8.8.8.8".into()),
    );
    let r2 = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx2,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_bool_true() {
    let mut ctx = HashMap::new();
    ctx.insert("aws:SecureTransport".to_string(), ContextValue::Bool(true));
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

#[test]
fn condition_bool_false_deny() {
    let mut ctx = HashMap::new();
    ctx.insert("aws:SecureTransport".to_string(), ContextValue::Bool(false));
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
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

// ── Policy variable substitution ─────────────────────────────────────────────

#[test]
fn substitutes_aws_username_in_resource() {
    // Policy says alice can read her own home directory.
    let policy = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::home/${aws:username}/*"}]}"#,
    )
    .unwrap();
    let identity = vec![policy];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let ctx = HashMap::new();

    // Alice reading her own object → allowed
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::home/alice/photo.jpg",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);

    // Alice reading bob's object → denied (substitution gives wrong path)
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::home/bob/photo.jpg",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn substitutes_principal_arn_in_condition() {
    // Allow s3:GetObject only when the resource tag matches the
    // requesting principal's ARN.
    let policy = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Action":"s3:GetObject",
            "Resource":"*",
            "Condition":{"StringEquals":{"aws:ResourceTag/Owner":"${aws:PrincipalArn}"}}
        }]}"#,
    )
    .unwrap();
    let identity = vec![policy];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };

    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:ResourceTag/Owner".to_string(),
        ContextValue::String("arn:aws:iam::1:user/alice".to_string()),
    );
    {
        let r = req(
            "arn:aws:iam::1:user/alice",
            "1",
            "s3:GetObject",
            "arn:aws:s3:::b/k",
            &ctx,
        );
        assert_eq!(evaluate(&r, &ec), Decision::Allow);
    }

    // Owner tag belongs to bob → implicit deny for alice
    ctx.insert(
        "aws:ResourceTag/Owner".to_string(),
        ContextValue::String("arn:aws:iam::1:user/bob".to_string()),
    );
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
fn unknown_policy_variable_left_literal() {
    // A typo in a policy variable should NOT silently widen the
    // policy. With the literal `${aws:notavar}` as the resource,
    // no real ARN can match.
    let policy = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::b/${aws:notavar}/*"}]}"#,
    )
    .unwrap();
    let identity = vec![policy];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let ctx = HashMap::new();
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/alice/photo.jpg",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

// Operator coverage: every BaseOperator the evaluator supports.

fn allow_with_condition(cond_json: &str) -> PolicyDocument {
    let doc = format!(
        r#"{{"Statement":[{{"Effect":"Allow","Action":"*","Resource":"*","Condition":{cond_json}}}]}}"#
    );
    parse(&doc).unwrap()
}

fn r_basic<'a>(ctx: &'a HashMap<String, ContextValue>) -> AuthzRequest<'a> {
    req(
        "arn:aws:iam::1:user/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        ctx,
    )
}

fn assert_decision(pol: PolicyDocument, ctx: &HashMap<String, ContextValue>, want: Decision) {
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    assert_eq!(evaluate(&r_basic(ctx), &ec), want);
}

#[test]
fn condition_string_not_equals() {
    let mut ctx = HashMap::new();
    ctx.insert("aws:username".into(), ContextValue::String("bob".into()));
    assert_decision(
        allow_with_condition(r#"{"StringNotEquals":{"aws:username":"alice"}}"#),
        &ctx,
        Decision::Allow,
    );
    ctx.insert("aws:username".into(), ContextValue::String("alice".into()));
    assert_decision(
        allow_with_condition(r#"{"StringNotEquals":{"aws:username":"alice"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

#[test]
fn condition_string_not_equals_ignore_case() {
    let mut ctx = HashMap::new();
    ctx.insert("aws:username".into(), ContextValue::String("ALICE".into()));
    assert_decision(
        allow_with_condition(r#"{"StringNotEqualsIgnoreCase":{"aws:username":"alice"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    ctx.insert("aws:username".into(), ContextValue::String("BOB".into()));
    assert_decision(
        allow_with_condition(r#"{"StringNotEqualsIgnoreCase":{"aws:username":"alice"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_string_not_like() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:userid".into(),
        ContextValue::String("AIDAEXAMPLEXYZ".into()),
    );
    assert_decision(
        allow_with_condition(r#"{"StringNotLike":{"aws:userid":"AIDA*"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    ctx.insert("aws:userid".into(), ContextValue::String("ROLEABCD".into()));
    assert_decision(
        allow_with_condition(r#"{"StringNotLike":{"aws:userid":"AIDA*"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_numeric_equals_and_not_equals() {
    let mut ctx = HashMap::new();
    ctx.insert("s3:max-keys".into(), ContextValue::Number(50.0));
    assert_decision(
        allow_with_condition(r#"{"NumericEquals":{"s3:max-keys":"50"}}"#),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(r#"{"NumericNotEquals":{"s3:max-keys":"50"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    assert_decision(
        allow_with_condition(r#"{"NumericNotEquals":{"s3:max-keys":"100"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_numeric_lte_gt_boundaries() {
    let mut ctx = HashMap::new();
    ctx.insert("s3:max-keys".into(), ContextValue::Number(100.0));
    assert_decision(
        allow_with_condition(r#"{"NumericLessThanEquals":{"s3:max-keys":"100"}}"#),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(r#"{"NumericLessThan":{"s3:max-keys":"100"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    assert_decision(
        allow_with_condition(r#"{"NumericGreaterThan":{"s3:max-keys":"50"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_date_equals_not_equals() {
    let mut ctx = HashMap::new();
    let dt: DateTime<Utc> = "2024-01-01T00:00:00Z".parse().unwrap();
    ctx.insert("aws:CurrentTime".into(), ContextValue::Date(dt));
    assert_decision(
        allow_with_condition(r#"{"DateEquals":{"aws:CurrentTime":"2024-01-01T00:00:00Z"}}"#),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(r#"{"DateNotEquals":{"aws:CurrentTime":"2024-01-01T00:00:00Z"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    assert_decision(
        allow_with_condition(r#"{"DateNotEquals":{"aws:CurrentTime":"2025-01-01T00:00:00Z"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_date_lte_gte_boundaries() {
    let mut ctx = HashMap::new();
    let dt: DateTime<Utc> = "2024-06-15T12:00:00Z".parse().unwrap();
    ctx.insert("aws:CurrentTime".into(), ContextValue::Date(dt));
    assert_decision(
        allow_with_condition(
            r#"{"DateLessThanEquals":{"aws:CurrentTime":"2024-06-15T12:00:00Z"}}"#,
        ),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(
            r#"{"DateGreaterThanEquals":{"aws:CurrentTime":"2024-06-15T12:00:00Z"}}"#,
        ),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(r#"{"DateLessThan":{"aws:CurrentTime":"2024-06-15T12:00:00Z"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

#[test]
fn condition_binary_equals_string_match() {
    let mut ctx = HashMap::new();
    // BinaryEquals does a string equality on the base64-encoded blob.
    ctx.insert("a:Bin".into(), ContextValue::String("aGVsbG8=".into()));
    assert_decision(
        allow_with_condition(r#"{"BinaryEquals":{"a:Bin":"aGVsbG8="}}"#),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(r#"{"BinaryEquals":{"a:Bin":"d29ybGQ="}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

#[test]
fn condition_arn_equals_and_not_equals() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SourceArn".into(),
        ContextValue::String("arn:aws:lambda:us-east-1:1:function:f".into()),
    );
    assert_decision(
        allow_with_condition(
            r#"{"ArnEquals":{"aws:SourceArn":"arn:aws:lambda:us-east-1:1:function:f"}}"#,
        ),
        &ctx,
        Decision::Allow,
    );
    assert_decision(
        allow_with_condition(
            r#"{"ArnNotEquals":{"aws:SourceArn":"arn:aws:lambda:us-east-1:1:function:f"}}"#,
        ),
        &ctx,
        Decision::ImplicitDeny,
    );
    assert_decision(
        allow_with_condition(
            r#"{"ArnNotEquals":{"aws:SourceArn":"arn:aws:lambda:us-east-1:1:function:other"}}"#,
        ),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_arn_not_like() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:SourceArn".into(),
        ContextValue::String("arn:aws:lambda:us-east-1:1:function:f".into()),
    );
    assert_decision(
        allow_with_condition(r#"{"ArnNotLike":{"aws:SourceArn":"arn:aws:lambda:*:1:function:*"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
    assert_decision(
        allow_with_condition(r#"{"ArnNotLike":{"aws:SourceArn":"arn:aws:s3:::*"}}"#),
        &ctx,
        Decision::Allow,
    );
}

// IfExists across base operators.

#[test]
fn condition_string_not_equals_if_exists_absent() {
    let ctx = HashMap::new();
    assert_decision(
        allow_with_condition(r#"{"StringNotEqualsIfExists":{"aws:username":"alice"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_numeric_less_than_if_exists_absent() {
    let ctx = HashMap::new();
    assert_decision(
        allow_with_condition(r#"{"NumericLessThanIfExists":{"s3:max-keys":"100"}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_ip_address_if_exists_present_must_match() {
    let mut ctx = HashMap::new();
    ctx.insert("aws:SourceIp".into(), ContextValue::Ip("1.2.3.4".into()));
    assert_decision(
        allow_with_condition(r#"{"IpAddressIfExists":{"aws:SourceIp":"10.0.0.0/8"}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

// Set qualifiers: edge cases.

#[test]
fn condition_for_any_value_no_overlap_denies() {
    let mut ctx = HashMap::new();
    ctx.insert(
        "aws:TagKeys".into(),
        ContextValue::StringList(vec!["env".into(), "team".into()]),
    );
    assert_decision(
        allow_with_condition(r#"{"ForAnyValue:StringEquals":{"aws:TagKeys":["cost","owner"]}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

#[test]
fn condition_for_all_values_empty_list_vacuously_allows() {
    // ForAllValues is vacuously true over an empty or missing multivalued
    // context key: every member of the empty set trivially matches. AWS
    // documents this behaviour (pair with a Null false check to avoid an
    // over-permissive Allow).
    let mut ctx = HashMap::new();
    ctx.insert("aws:TagKeys".into(), ContextValue::StringList(Vec::new()));
    assert_decision(
        allow_with_condition(r#"{"ForAllValues:StringEquals":{"aws:TagKeys":["env"]}}"#),
        &ctx,
        Decision::Allow,
    );
}

#[test]
fn condition_for_any_value_missing_key() {
    // Missing context key with ForAnyValue: no values to test → deny.
    let ctx = HashMap::new();
    assert_decision(
        allow_with_condition(r#"{"ForAnyValue:StringEquals":{"aws:TagKeys":["env"]}}"#),
        &ctx,
        Decision::ImplicitDeny,
    );
}

#[test]
fn condition_block_multiple_keys_all_must_match() {
    // Two condition operators in one Condition block: AND.
    let mut ctx = HashMap::new();
    ctx.insert("aws:username".into(), ContextValue::String("alice".into()));
    ctx.insert("aws:SourceIp".into(), ContextValue::Ip("10.0.0.5".into()));
    let pol_match = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*","Condition":{
            "StringEquals":{"aws:username":"alice"},
            "IpAddress":{"aws:SourceIp":"10.0.0.0/8"}
        }}]}"#,
    )
    .unwrap();
    assert_decision(pol_match, &ctx, Decision::Allow);

    let pol_one_fails = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*","Condition":{
            "StringEquals":{"aws:username":"alice"},
            "IpAddress":{"aws:SourceIp":"192.168.0.0/16"}
        }}]}"#,
    )
    .unwrap();
    assert_decision(pol_one_fails, &ctx, Decision::ImplicitDeny);
}

// NotResource semantics.

#[test]
fn not_resource_inversion() {
    let ctx = HashMap::new();
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","NotResource":"arn:aws:s3:::secret/*"}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let allowed = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::public/k",
        &ctx,
    );
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::secret/k",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn deny_with_not_action_blocks_everything_outside_set() {
    // Deny + NotAction: deny every action *except* the listed one.
    // Combined with admin Allow: only s3:GetObject survives.
    let ctx = HashMap::new();
    let allow = admin();
    let deny =
        parse(r#"{"Statement":[{"Effect":"Deny","NotAction":"s3:GetObject","Resource":"*"}]}"#)
            .unwrap();
    let identity = vec![allow, deny];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let get = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&get, &ec), Decision::Allow);
    let put = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:PutObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&put, &ec), Decision::ExplicitDeny);
}

#[test]
fn not_principal_in_resource_policy() {
    // Allow everyone except a specific user.
    let ctx = HashMap::new();
    let bucket = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "NotPrincipal":{"AWS":"arn:aws:iam::1:user/blocked"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        resource_policy: Some(&bucket),
        ..Default::default()
    };
    let alice = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&alice, &ec), Decision::Allow);
    let blocked = req(
        "arn:aws:iam::1:user/blocked",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&blocked, &ec), Decision::ImplicitDeny);
}

// Permissions boundary / session policy explicit deny.

#[test]
fn boundary_explicit_deny_overrides_admin() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let boundary =
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"iam:*","Resource":"*"}]}"#).unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
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
fn session_explicit_deny_overrides_admin() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let session =
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"s3:DeleteObject","Resource":"*"}]}"#)
            .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        session_policy: Some(&session),
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
fn boundary_does_not_grant_without_identity_allow() {
    // Boundary alone cannot grant. Identity has nothing → implicit deny
    // even though the boundary contains an Allow.
    let ctx = HashMap::new();
    let identity: Vec<PolicyDocument> = Vec::new();
    let boundary = admin();
    let ec = EvalContext {
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
        ..Default::default()
    };
    let r = r_basic(&ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn scp_does_not_grant_without_identity_allow() {
    // SCP alone cannot grant either.
    let ctx = HashMap::new();
    let scps = vec![admin()];
    let ec = EvalContext {
        scps: &scps,
        ..Default::default()
    };
    let r = r_basic(&ctx);
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

#[test]
fn multiple_scps_all_must_allow() {
    // Two SCPs in scope. The action must be allowed in *every* one
    // (intersection), so missing in one → implicit deny.
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let scp_s3 =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#).unwrap();
    let scp_ec2 =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"ec2:*","Resource":"*"}]}"#).unwrap();
    let scps = vec![scp_s3, scp_ec2];
    let ec = EvalContext {
        identity_policies: &identity,
        scps: &scps,
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::ImplicitDeny);
}

// Combined contexts.

#[test]
fn identity_plus_boundary_plus_session_intersection() {
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let boundary = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":["s3:GetObject","s3:PutObject"],"Resource":"*"}]}"#,
    )
    .unwrap();
    let session =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#)
            .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
        session_policy: Some(&session),
        ..Default::default()
    };
    let get = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&get, &ec), Decision::Allow);
    // Allowed by identity + boundary but not by session → implicit deny.
    let put = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:PutObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&put, &ec), Decision::ImplicitDeny);
}

#[test]
fn full_stack_identity_boundary_session_scp_resource() {
    // Identity admin, boundary s3:*, session s3:Get*, SCP s3:*, bucket
    // policy allows GetObject for alice. Cross-cutting AND yields:
    // s3:GetObject allowed, s3:PutObject denied (session strips it).
    let ctx = HashMap::new();
    let identity = vec![admin()];
    let boundary =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#).unwrap();
    let session =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:Get*","Resource":"*"}]}"#).unwrap();
    let scps = vec![
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#).unwrap(),
    ];
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
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
        session_policy: Some(&session),
        scps: &scps,
        resource_policy: Some(&bucket_policy),
    };
    let get = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&get, &ec), Decision::Allow);
    let put = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:PutObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    assert_eq!(evaluate(&put, &ec), Decision::ImplicitDeny);
}

#[test]
fn cross_account_resource_allow_satisfies_boundary_short_circuit() {
    // Alice is in account 222 with a permissions-boundary that does
    // NOT allow s3:GetObject; the bucket (account 111) explicitly
    // grants alice. The evaluator's boundary-shortcut gates on the
    // resource policy granting; cross-account requires the identity
    // also to allow.
    let ctx = HashMap::new();
    let identity = vec![
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#)
            .unwrap(),
    ];
    let boundary = parse(
        // boundary doesn't include s3
        r#"{"Statement":[{"Effect":"Allow","Action":"ec2:*","Resource":"*"}]}"#,
    )
    .unwrap();
    let bucket = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::222:user/alice"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let ec = EvalContext {
        identity_policies: &identity,
        permissions_boundary: Some(&boundary),
        resource_policy: Some(&bucket),
        ..Default::default()
    };
    let r = req(
        "arn:aws:iam::222:user/alice",
        "222",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    // The resource-policy short-circuit means the boundary does not
    // need to allow the action when the resource policy grants.
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

// ${aws:PrincipalAccount} substitution and literal ${$}/${*}/${?}.

#[test]
fn substitutes_principal_account() {
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::acct-${aws:PrincipalAccount}/*"}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let ctx = HashMap::new();
    let allowed = req(
        "arn:aws:iam::123456789012:user/alice",
        "123456789012",
        "s3:GetObject",
        "arn:aws:s3:::acct-123456789012/k",
        &ctx,
    );
    assert_eq!(evaluate(&allowed, &ec), Decision::Allow);
    let denied = req(
        "arn:aws:iam::123456789012:user/alice",
        "123456789012",
        "s3:GetObject",
        "arn:aws:s3:::acct-999999999999/k",
        &ctx,
    );
    assert_eq!(evaluate(&denied, &ec), Decision::ImplicitDeny);
}

#[test]
fn literal_special_var_escapes() {
    // ${*}, ${?}, ${$} should emit `*`, `?`, `$` literally so that
    // glob metachars in resource templates can be quoted.
    let pol = parse(
        r#"{"Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::lit/${*}/${?}/${$}/k"}]}"#,
    )
    .unwrap();
    let identity = vec![pol];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let ctx = HashMap::new();
    // After substitution: arn:aws:s3:::lit/*/?/$/k. Because matches_arn
    // treats `*` and `?` as glob metachars in patterns, the path
    // segments `whatever`, `a`, and `$` should match.
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::lit/whatever/a/$/k",
        &ctx,
    );
    assert_eq!(evaluate(&r, &ec), Decision::Allow);
}

// evaluate_detailed: matched statements + missing context keys.

#[test]
fn evaluate_detailed_reports_matched_statements() {
    let identity = vec![
        admin(),
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"s3:DeleteObject","Resource":"*"}]}"#)
            .unwrap(),
    ];
    let attrs = vec![
        PolicyAttribution {
            source_id: "AdminAccess".into(),
            source_type: PolicySource::Identity,
        },
        PolicyAttribution {
            source_id: "DenyDelete".into(),
            source_type: PolicySource::Identity,
        },
    ];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let pa = PolicyAttributions {
        identity: &attrs,
        ..Default::default()
    };
    let ctx = HashMap::new();
    let r = req(
        "arn:aws:iam::1:u/x",
        "1",
        "s3:DeleteObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    let details = evaluate_detailed(&r, &ec, &pa);
    assert_eq!(details.decision, Decision::ExplicitDeny);
    let ids: Vec<&str> = details
        .matched_statements
        .iter()
        .map(|m| m.source_id.as_str())
        .collect();
    assert!(ids.contains(&"AdminAccess"));
    assert!(ids.contains(&"DenyDelete"));
}

#[test]
fn evaluate_detailed_reports_missing_context_keys() {
    let identity = vec![
        parse(
            r#"{"Statement":[{"Effect":"Allow","Action":"*","Resource":"*",
            "Condition":{"StringEquals":{"aws:username":"alice","aws:RequestTag/team":"core"}}}]}"#,
        )
        .unwrap(),
    ];
    let attrs = vec![PolicyAttribution {
        source_id: "Pol".into(),
        source_type: PolicySource::Identity,
    }];
    let ec = EvalContext {
        identity_policies: &identity,
        ..Default::default()
    };
    let pa = PolicyAttributions {
        identity: &attrs,
        ..Default::default()
    };
    let ctx = HashMap::new();
    let details = evaluate_detailed(&r_basic(&ctx), &ec, &pa);
    assert_eq!(details.decision, Decision::ImplicitDeny);
    assert!(
        details
            .missing_context_values
            .contains(&"aws:username".to_string())
    );
    assert!(
        details
            .missing_context_values
            .contains(&"aws:RequestTag/team".to_string())
    );
}

#[test]
fn evaluate_detailed_resource_policy_attribution() {
    let bucket = parse(
        r#"{"Statement":[{
            "Effect":"Allow",
            "Principal":{"AWS":"arn:aws:iam::1:user/alice"},
            "Action":"s3:GetObject",
            "Resource":"arn:aws:s3:::b/*"
        }]}"#,
    )
    .unwrap();
    let attr = PolicyAttribution {
        source_id: "BucketPolicy".into(),
        source_type: PolicySource::Resource,
    };
    let ec = EvalContext {
        resource_policy: Some(&bucket),
        ..Default::default()
    };
    let pa = PolicyAttributions {
        resource: Some(&attr),
        ..Default::default()
    };
    let ctx = HashMap::new();
    let r = req(
        "arn:aws:iam::1:user/alice",
        "1",
        "s3:GetObject",
        "arn:aws:s3:::b/k",
        &ctx,
    );
    let details = evaluate_detailed(&r, &ec, &pa);
    assert_eq!(details.decision, Decision::Allow);
    assert_eq!(details.matched_statements.len(), 1);
    assert_eq!(details.matched_statements[0].source_id, "BucketPolicy");
    assert_eq!(
        details.matched_statements[0].source_type,
        PolicySource::Resource
    );
}

// `explain` must never contradict `evaluate`: the reason's implied
// decision has to equal the decision across the whole pipeline.

fn reason_decision(r: &DecisionReason) -> Decision {
    match r {
        DecisionReason::ExplicitDeny { .. } => Decision::ExplicitDeny,
        DecisionReason::Allowed { .. } => Decision::Allow,
        DecisionReason::ScpImplicitDeny { .. }
        | DecisionReason::NoAllow
        | DecisionReason::BoundaryNoAllow
        | DecisionReason::SessionNoAllow => Decision::ImplicitDeny,
    }
}

#[test]
fn explain_agrees_with_evaluate_across_pipeline() {
    let ctx = HashMap::new();
    let r = r_basic(&ctx);

    let deny_s3 =
        parse(r#"{"Statement":[{"Effect":"Deny","Action":"s3:*","Resource":"*"}]}"#).unwrap();
    let scp_no_s3 =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"ec2:*","Resource":"*"}]}"#).unwrap();
    let boundary_no_s3 =
        parse(r#"{"Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#).unwrap();
    let admin_pol = admin();

    // (label, identity, boundary, scps) tuples covering each branch.
    let id_admin = vec![admin_pol.clone()];
    let id_admin_deny = vec![admin_pol.clone(), deny_s3.clone()];
    let id_empty: Vec<PolicyDocument> = vec![];
    let scps_ok = vec![admin_pol.clone()];
    let scps_block = vec![scp_no_s3.clone()];

    type Case<'a> = (
        &'a str,
        &'a [PolicyDocument],
        Option<&'a PolicyDocument>,
        &'a [PolicyDocument],
    );
    let cases: Vec<Case> = vec![
        ("allow", &id_admin, None, &[]),
        ("explicit-deny", &id_admin_deny, None, &[]),
        ("no-allow", &id_empty, None, &[]),
        ("scp-implicit-deny", &id_admin, None, &scps_block),
        ("scp-allowed", &id_admin, None, &scps_ok),
        ("boundary-blocks", &id_admin, Some(&boundary_no_s3), &[]),
    ];

    for (label, identity, boundary, scps) in cases {
        let ec = EvalContext {
            identity_policies: identity,
            permissions_boundary: boundary,
            scps,
            ..Default::default()
        };
        let pa = PolicyAttributions::default();
        let decision = evaluate(&r, &ec);
        let reason = explain(&r, &ec, &pa);
        let details = evaluate_detailed(&r, &ec, &pa);
        assert_eq!(
            reason_decision(&reason),
            decision,
            "explain disagreed with evaluate for case `{label}`: {reason:?} vs {decision:?}"
        );
        assert_eq!(
            details.decision, decision,
            "evaluate_detailed.decision drifted for `{label}`"
        );
        assert_eq!(
            reason_decision(&details.reason),
            decision,
            "evaluate_detailed.reason drifted for `{label}`"
        );
    }
}
