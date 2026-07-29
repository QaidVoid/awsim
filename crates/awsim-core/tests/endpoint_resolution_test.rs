//! Resource URLs handed back to a caller must point somewhere that caller
//! can actually reach. A hardcoded endpoint is wrong the moment AWSim runs
//! on another port, under Testcontainers (random mapped port), or is
//! addressed by container name from a sibling container.

use awsim_core::RequestContext;

fn ctx_with(authority: Option<&str>, secure: bool) -> RequestContext {
    let mut ctx = RequestContext::new("sqs", "us-east-1");
    ctx.endpoint_authority = authority.map(|s| s.to_string());
    ctx.is_secure = secure;
    ctx
}

#[test]
fn base_url_uses_resolved_authority() {
    let ctx = ctx_with(Some("localhost:4599"), false);
    assert_eq!(ctx.base_url(), "http://localhost:4599");
}

#[test]
fn base_url_falls_back_when_unresolved() {
    // Unit tests and server-internal contexts have no request to resolve
    // from. The historical default keeps them working unchanged.
    let ctx = ctx_with(None, false);
    assert_eq!(ctx.base_url(), "http://localhost:4566");
}

#[test]
fn scheme_follows_transport() {
    assert_eq!(
        ctx_with(Some("example:443"), true).base_url(),
        "https://example:443"
    );
    assert_eq!(
        ctx_with(Some("example:80"), false).base_url(),
        "http://example:80"
    );
}

#[test]
fn service_base_url_keeps_the_aws_subdomain_shape() {
    // Mirrors sqs.us-east-1.amazonaws.com. Some clients parse the region
    // back out of a queue URL, so the shape is worth preserving.
    let ctx = ctx_with(Some("localhost:4599"), false);
    assert_eq!(
        ctx.service_base_url("sqs"),
        "http://sqs.us-east-1.localhost:4599"
    );
}

#[test]
fn service_base_url_falls_back_for_ip_hosts() {
    // `sqs.us-east-1.127.0.0.1` does not resolve, so an IP authority must
    // drop the subdomain rather than emit something unusable.
    for ip in ["127.0.0.1:4566", "10.0.0.5:4566"] {
        let ctx = ctx_with(Some(ip), false);
        assert_eq!(
            ctx.service_base_url("sqs"),
            format!("http://{ip}"),
            "IP authority {ip} should not carry a service subdomain"
        );
    }
}

#[test]
fn service_base_url_handles_container_names() {
    // Docker Compose: a sibling container reaches AWSim by service name.
    let ctx = ctx_with(Some("awsim:4566"), false);
    assert_eq!(
        ctx.service_base_url("sqs"),
        "http://sqs.us-east-1.awsim:4566"
    );
}

#[test]
fn authority_without_port_is_supported() {
    let ctx = ctx_with(Some("aws.example.com"), true);
    assert_eq!(ctx.base_url(), "https://aws.example.com");
    assert_eq!(
        ctx.service_base_url("sqs"),
        "https://sqs.us-east-1.aws.example.com"
    );
}
