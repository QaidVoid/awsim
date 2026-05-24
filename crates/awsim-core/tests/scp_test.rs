use std::sync::Arc;

use awsim_core::{AuthzEngine, PrincipalLookup, RequestContext, ResolvedPrincipal, ScpLookup};
use awsim_iam_policy::PolicyDocument;

struct StubPrincipal {
    principal: ResolvedPrincipal,
}

impl PrincipalLookup for StubPrincipal {
    fn resolve_access_key(&self, _access_key: &str) -> Option<ResolvedPrincipal> {
        Some(ResolvedPrincipal {
            arn: self.principal.arn.clone(),
            account: self.principal.account.clone(),
            identity_policies: self.principal.identity_policies.clone(),
            permissions_boundary: self.principal.permissions_boundary.clone(),
            is_root: self.principal.is_root,
            tags: Default::default(),
            session_policy: None,
        })
    }
}

struct StubScpLookup {
    policies: Vec<PolicyDocument>,
}

impl ScpLookup for StubScpLookup {
    fn lookup(&self, _principal_arn: &str) -> Vec<PolicyDocument> {
        self.policies.clone()
    }
}

fn make_principal(docs: Vec<&str>) -> ResolvedPrincipal {
    let identity_policies: Vec<PolicyDocument> = docs
        .into_iter()
        .map(|d| awsim_iam_policy::parse(d).expect("policy parses"))
        .collect();
    ResolvedPrincipal {
        arn: "arn:aws:iam::000000000000:user/Dana".to_string(),
        account: "000000000000".to_string(),
        identity_policies,
        permissions_boundary: None,
        is_root: false,
        tags: Default::default(),
        session_policy: None,
    }
}

fn make_ctx() -> RequestContext {
    let mut ctx = RequestContext::new("s3", "us-east-1");
    ctx.access_key = Some("AKIATEST".to_string());
    ctx.account_id = "000000000000".to_string();
    ctx
}

fn engine(principal: ResolvedPrincipal, scp: Option<Arc<dyn ScpLookup>>) -> AuthzEngine {
    let mut e = AuthzEngine::new(true);
    e.principal_lookup = Arc::new(StubPrincipal { principal });
    e.scp_lookup = scp;
    e
}

const ALLOW_S3_AND_DDB: &str = r#"{
    "Version": "2012-10-17",
    "Statement": [{
        "Effect": "Allow",
        "Action": ["s3:GetObject", "dynamodb:PutItem"],
        "Resource": "*"
    }]
}"#;

const SCP_ALLOW_ONLY_S3: &str = r#"{
    "Version": "2012-10-17",
    "Statement": [{
        "Effect": "Allow",
        "Action": "s3:*",
        "Resource": "*"
    }]
}"#;

const SCP_DENY_S3: &str = r#"{
    "Version": "2012-10-17",
    "Statement": [{
        "Effect": "Deny",
        "Action": "s3:GetObject",
        "Resource": "*"
    }]
}"#;

#[test]
fn no_scp_identity_allow_passes() {
    let p = make_principal(vec![ALLOW_S3_AND_DDB]);
    let eng = engine(p, None);
    let ctx = make_ctx();
    assert!(
        eng.check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn scp_allows_only_s3_dynamodb_call_denied() {
    let p = make_principal(vec![ALLOW_S3_AND_DDB]);
    let scp_policy = awsim_iam_policy::parse(SCP_ALLOW_ONLY_S3).unwrap();
    let scp: Arc<dyn ScpLookup> = Arc::new(StubScpLookup {
        policies: vec![scp_policy],
    });
    let eng = engine(p, Some(scp));
    let ctx = make_ctx();
    let err = eng
        .check(
            &ctx,
            "dynamodb:PutItem",
            "arn:aws:dynamodb:us-east-1:000000000000:table/T",
        )
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}

#[test]
fn scp_allows_only_s3_s3_call_allowed() {
    let p = make_principal(vec![ALLOW_S3_AND_DDB]);
    let scp_policy = awsim_iam_policy::parse(SCP_ALLOW_ONLY_S3).unwrap();
    let scp: Arc<dyn ScpLookup> = Arc::new(StubScpLookup {
        policies: vec![scp_policy],
    });
    let eng = engine(p, Some(scp));
    let ctx = make_ctx();
    assert!(
        eng.check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
            .is_ok()
    );
}

#[test]
fn scp_explicit_deny_overrides_identity_allow() {
    let p = make_principal(vec![ALLOW_S3_AND_DDB]);
    let scp_policy = awsim_iam_policy::parse(SCP_DENY_S3).unwrap();
    let scp: Arc<dyn ScpLookup> = Arc::new(StubScpLookup {
        policies: vec![scp_policy],
    });
    let eng = engine(p, Some(scp));
    let ctx = make_ctx();
    let err = eng
        .check(&ctx, "s3:GetObject", "arn:aws:s3:::bucket/key")
        .unwrap_err();
    assert_eq!(err.code, "AccessDenied");
}
