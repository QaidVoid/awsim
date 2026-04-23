use std::sync::Arc;

use awsim_core::{
    AuthzEngine, NoopPrincipalLookup, PrincipalLookup, RequestContext, ResolvedPrincipal,
};
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
        })
    }
}

fn make_principal(
    arn: &str,
    account: &str,
    docs: Vec<&str>,
    is_root: bool,
) -> ResolvedPrincipal {
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
    let engine = AuthzEngine {
        principal_lookup: Arc::new(NoopPrincipalLookup),
        resource_policy_lookups: Default::default(),
        scp_lookup: None,
        enabled: false,
    };
    let ctx = ctx_with_key(None);
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn enforcement_on_anonymous_denied() {
    let engine = AuthzEngine {
        principal_lookup: Arc::new(NoopPrincipalLookup),
        resource_policy_lookups: Default::default(),
        scp_lookup: None,
        enabled: true,
    };
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
    let engine = AuthzEngine {
        principal_lookup: Arc::new(StubLookup {
            principal: Some(principal),
        }),
        resource_policy_lookups: Default::default(),
        scp_lookup: None,
        enabled: true,
    };
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
    let engine = AuthzEngine {
        principal_lookup: Arc::new(StubLookup {
            principal: Some(principal),
        }),
        resource_policy_lookups: Default::default(),
        scp_lookup: None,
        enabled: true,
    };
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
    };
    let engine = AuthzEngine {
        principal_lookup: Arc::new(StubLookup {
            principal: Some(principal),
        }),
        resource_policy_lookups: Default::default(),
        scp_lookup: None,
        enabled: true,
    };
    let ctx = ctx_with_key(Some("AKIAROOT"));
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}
