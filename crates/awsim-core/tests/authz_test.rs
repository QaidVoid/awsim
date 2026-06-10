use std::sync::Arc;

use awsim_core::{AuthzEngine, PrincipalLookup, RequestContext, ResolvedPrincipal};
use awsim_iam_policy::PolicyDocument;

struct StubLookup {
    principal: Option<ResolvedPrincipal>,
}

impl PrincipalLookup for StubLookup {
    fn resolve_access_key(&self, _access_key: &str) -> Option<ResolvedPrincipal> {
        self.principal.as_ref().map(|p| ResolvedPrincipal {
            arn: p.arn.clone(),
            account: p.account.clone(),
            identity_policies: p.identity_policies.clone(),
            permissions_boundary: p.permissions_boundary.clone(),
            is_root: p.is_root,
            tags: p.tags.clone(),
            session_policy: None,
        })
    }
}

fn make_principal(arn: &str, account: &str, docs: Vec<&str>, is_root: bool) -> ResolvedPrincipal {
    let identity_policies: Vec<PolicyDocument> = docs
        .into_iter()
        .map(|d| awsim_iam_policy::parse(d).expect("policy parses"))
        .collect();
    ResolvedPrincipal {
        arn: arn.to_string(),
        account: account.to_string(),
        identity_policies,
        permissions_boundary: None,
        is_root,
        tags: Default::default(),
        session_policy: None,
    }
}

fn ctx_with_key(key: Option<&str>) -> RequestContext {
    let mut ctx = RequestContext::new("s3", "us-east-1");
    ctx.access_key = key.map(|k| k.to_string());
    ctx.account_id = "000000000000".to_string();
    ctx
}

#[test]
fn enforcement_off_passes() {
    let engine = AuthzEngine::new(false);
    let ctx = ctx_with_key(None);
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn enforcement_on_anonymous_denied() {
    let engine = AuthzEngine::new(true);
    let ctx = ctx_with_key(None);
    let err = engine
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}

#[test]
fn enforcement_on_allow_passes() {
    let allow_policy = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::bucket/*"
        }]
    }"#;
    let principal = make_principal(
        "arn:aws:iam::000000000000:user/Alice",
        "000000000000",
        vec![allow_policy],
        false,
    );
    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    let ctx = ctx_with_key(Some("AKIATEST"));
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn enforcement_on_deny_blocks() {
    let deny_policy = r#"{
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": "s3:*",
                "Resource": "*"
            },
            {
                "Effect": "Deny",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::bucket/*"
            }
        ]
    }"#;
    let principal = make_principal(
        "arn:aws:iam::000000000000:user/Bob",
        "000000000000",
        vec![deny_policy],
        false,
    );
    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    let ctx = ctx_with_key(Some("AKIATEST"));
    let err = engine
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}

#[test]
fn enforcement_on_root_bypass() {
    let principal = ResolvedPrincipal {
        arn: "arn:aws:iam::000000000000:root".to_string(),
        account: "000000000000".to_string(),
        identity_policies: vec![],
        permissions_boundary: None,
        is_root: true,
        tags: Default::default(),
        session_policy: None,
    };
    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    let ctx = ctx_with_key(Some("AKIAROOT"));
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn ip_address_condition_uses_aws_source_ip() {
    // Identity policy that only allows when the caller's IP matches
    // 10.0.0.0/8. Without aws:SourceIp populated this would never match.
    let policy = r#"{
        "Version":"2012-10-17",
        "Statement":[{
            "Effect":"Allow",
            "Action":"s3:GetObject",
            "Resource":"*",
            "Condition":{"IpAddress":{"aws:SourceIp":["10.0.0.0/8"]}}
        }]
    }"#;
    let principal = make_principal(
        "arn:aws:iam::000000000000:user/u",
        "000000000000",
        vec![policy],
        false,
    );
    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    let mut ctx = ctx_with_key(Some("AKIA"));
    ctx.source_ip = Some("10.1.2.3".to_string());
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok(),
        "policy should match in-network source IP"
    );

    // Out-of-network IP must fall to implicit deny.
    ctx.source_ip = Some("203.0.113.1".to_string());
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_err()
    );
}

#[test]
fn principal_tag_condition_consumes_resolved_tags() {
    let policy = r#"{
        "Version":"2012-10-17",
        "Statement":[{
            "Effect":"Allow",
            "Action":"s3:GetObject",
            "Resource":"*",
            "Condition":{"StringEquals":{"aws:PrincipalTag/team":"infra"}}
        }]
    }"#;
    let mut principal = make_principal(
        "arn:aws:iam::000000000000:user/u",
        "000000000000",
        vec![policy],
        false,
    );
    principal.tags.insert("team".into(), "infra".into());

    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    let ctx = ctx_with_key(Some("AKIA"));
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok(),
        "principal with team=infra tag should match"
    );
}

#[test]
fn secure_transport_condition_reflects_ctx_flag() {
    let policy = r#"{
        "Version":"2012-10-17",
        "Statement":[{
            "Effect":"Deny",
            "Action":"*",
            "Resource":"*",
            "Condition":{"Bool":{"aws:SecureTransport":"false"}}
        },
        {
            "Effect":"Allow",
            "Action":"s3:GetObject",
            "Resource":"*"
        }]
    }"#;
    let principal = make_principal(
        "arn:aws:iam::000000000000:user/u",
        "000000000000",
        vec![policy],
        false,
    );
    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    // Plain HTTP: explicit Deny fires.
    let mut ctx = ctx_with_key(Some("AKIA"));
    ctx.is_secure = false;
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::b/k")
            .is_err()
    );
    // HTTPS: deny doesn't apply, allow takes over.
    ctx.is_secure = true;
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::b/k")
            .is_ok()
    );
}

/// A `PrincipalLookup` that panics on use, so the test fails loudly
/// if the admin-key short-circuit ever consults it.
struct PanicLookup;
impl PrincipalLookup for PanicLookup {
    fn resolve_access_key(&self, _access_key: &str) -> Option<ResolvedPrincipal> {
        panic!("admin key must bypass principal_lookup");
    }
}

#[test]
fn admin_access_key_bypasses_enforcement() {
    let mut engine = AuthzEngine::new(true);
    engine.admin_access_key = Some("awsim-admin".to_string());
    engine.principal_lookup = Arc::new(PanicLookup);
    let ctx = ctx_with_key(Some("awsim-admin"));
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn admin_access_key_does_not_bypass_other_keys() {
    let mut engine = AuthzEngine::new(true);
    engine.admin_access_key = Some("awsim-admin".to_string());
    engine.principal_lookup = Arc::new(StubLookup { principal: None });
    let ctx = ctx_with_key(Some("not-admin"));
    let err = engine
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "InvalidClientTokenId");
    assert!(
        !err.message.contains("not-admin"),
        "error message must not echo the access key: {}",
        err.message
    );
}

#[test]
fn admin_access_key_unset_does_not_match_empty_key() {
    let engine = AuthzEngine::new(true);
    assert!(engine.admin_access_key.is_none());
    let ctx = ctx_with_key(Some(""));
    let err = engine
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}
