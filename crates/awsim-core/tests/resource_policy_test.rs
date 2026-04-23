use std::sync::Arc;

use awsim_core::{
    AuthzEngine, PrincipalLookup, RequestContext, ResolvedPrincipal, ResourcePolicyLookup,
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

struct StubResourcePolicyLookup {
    policy: PolicyDocument,
    expected_arn: String,
}

impl ResourcePolicyLookup for StubResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        if resource_arn == self.expected_arn
            || resource_arn.starts_with(&self.expected_arn.trim_end_matches('*').to_string())
        {
            Some(self.policy.clone())
        } else {
            None
        }
    }
}

fn ctx_with_key(service: &str, key: &str) -> RequestContext {
    let mut ctx = RequestContext::new(service, "us-east-1");
    ctx.access_key = Some(key.to_string());
    ctx.account_id = "000000000000".to_string();
    ctx
}

#[test]
fn resource_policy_grants_access_when_identity_has_none() {
    let principal = ResolvedPrincipal {
        arn: "arn:aws:iam::000000000000:user/Carol".to_string(),
        account: "000000000000".to_string(),
        identity_policies: vec![],
        permissions_boundary: None,
        is_root: false,
    };

    let resource_policy = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::000000000000:user/Carol"},
            "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::bucket/*"
        }]
    }"#;
    let doc = awsim_iam_policy::parse(resource_policy).expect("policy parses");

    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    engine.resource_policy_lookups.insert(
        "s3".to_string(),
        Arc::new(StubResourcePolicyLookup {
            policy: doc,
            expected_arn: "arn:aws:s3:::bucket/*".to_string(),
        }),
    );

    let ctx = ctx_with_key("s3", "AKIATEST");
    assert!(
        engine
            .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn resource_policy_only_used_for_matching_service() {
    let principal = ResolvedPrincipal {
        arn: "arn:aws:iam::000000000000:user/Dan".to_string(),
        account: "000000000000".to_string(),
        identity_policies: vec![],
        permissions_boundary: None,
        is_root: false,
    };

    let resource_policy = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::000000000000:user/Dan"},
            "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::bucket/*"
        }]
    }"#;
    let doc = awsim_iam_policy::parse(resource_policy).expect("policy parses");

    let mut engine = AuthzEngine::new(true);
    engine.principal_lookup = Arc::new(StubLookup {
        principal: Some(principal),
    });
    engine.resource_policy_lookups.insert(
        "kms".to_string(),
        Arc::new(StubResourcePolicyLookup {
            policy: doc,
            expected_arn: "arn:aws:s3:::bucket/*".to_string(),
        }),
    );

    let ctx = ctx_with_key("s3", "AKIATEST");
    let err = engine
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}
