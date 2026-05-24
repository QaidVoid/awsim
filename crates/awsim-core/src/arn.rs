//! AWS ARN construction and parsing.
//!
//! AWS resource names follow the format
//! `arn:<partition>:<service>:<region>:<account>:<resource>` where the
//! partition is one of `aws`, `aws-cn`, `aws-us-gov`, or `aws-iso(-b)`.
//! Some services (IAM, Route 53, Organizations, CloudFront, STS at the
//! global endpoint) leave the region segment empty; some services (S3
//! bucket ARNs) also leave the account segment empty.
//!
//! This module provides a single helper that pulls partition, region,
//! and account from a [`RequestContext`] so individual services don't
//! have to remember to thread those fields through. A non-default
//! [`AWSIM_PARTITION`] / [`AWSIM_REGION`] / [`AWSIM_ACCOUNT_ID`] is
//! reflected in every emitted ARN.
//!
//! [`AWSIM_PARTITION`]: https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html
//! [`AWSIM_REGION`]: https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html
//! [`AWSIM_ACCOUNT_ID`]: https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html

use crate::error::AwsError;
use crate::router::RequestContext;
use std::fmt;

/// Parsed AWS ARN with each segment held separately.
///
/// Use [`build`] / [`build_global`] to create one from a request
/// context, or [`parse`] to decompose an external string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arn {
    pub partition: String,
    pub service: String,
    pub region: String,
    pub account: String,
    pub resource: String,
}

impl fmt::Display for Arn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "arn:{}:{}:{}:{}:{}",
            self.partition, self.service, self.region, self.account, self.resource
        )
    }
}

/// Build a regional ARN for `service` and `resource` using the
/// request's partition, region, and account.
pub fn build(ctx: &RequestContext, service: &'static str, resource: impl AsRef<str>) -> String {
    format!(
        "arn:{}:{}:{}:{}:{}",
        ctx.partition,
        service,
        ctx.region,
        ctx.account_id,
        resource.as_ref()
    )
}

/// Build a global-service ARN (IAM, Route 53, CloudFront, ...): the
/// region segment is empty, the account is preserved.
pub fn build_global(
    ctx: &RequestContext,
    service: &'static str,
    resource: impl AsRef<str>,
) -> String {
    format!(
        "arn:{}:{}::{}:{}",
        ctx.partition,
        service,
        ctx.account_id,
        resource.as_ref()
    )
}

/// Build a partition-only ARN (S3 buckets: no region, no account).
pub fn build_partition(
    ctx: &RequestContext,
    service: &'static str,
    resource: impl AsRef<str>,
) -> String {
    format!("arn:{}:{}:::{}", ctx.partition, service, resource.as_ref())
}

/// Parse an external ARN string into its segments.
///
/// Rejects strings that don't have the required six colon-separated
/// fields with `InvalidParameterValue`. Per-service validation
/// (allowed services, resource shape) is the caller's responsibility.
pub fn parse(s: &str) -> Result<Arn, AwsError> {
    // ARNs are `arn:partition:service:region:account:resource`. The
    // `resource` segment itself may contain colons (e.g.,
    // `arn:aws:logs:us-east-1:111:log-group:/aws/lambda/foo:log-stream:bar`)
    // so split into the first 6 parts and keep the remainder verbatim.
    let mut it = s.splitn(6, ':');
    let scheme = it.next();
    let partition = it.next();
    let service = it.next();
    let region = it.next();
    let account = it.next();
    let resource = it.next();
    match (scheme, partition, service, region, account, resource) {
        (Some("arn"), Some(p), Some(s), Some(r), Some(a), Some(res))
            if !p.is_empty() && !s.is_empty() =>
        {
            Ok(Arn {
                partition: p.to_string(),
                service: s.to_string(),
                region: r.to_string(),
                account: a.to_string(),
                resource: res.to_string(),
            })
        }
        _ => Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Malformed ARN: {s}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(partition: &str, region: &str, account: &str) -> RequestContext {
        let mut ctx = RequestContext::new_with_account("ec2", region, account);
        ctx.partition = partition.to_string();
        ctx
    }

    #[test]
    fn build_uses_request_context_segments() {
        let ctx = ctx_with("aws", "us-east-1", "111122223333");
        assert_eq!(
            build(&ctx, "ec2", "instance/i-abc"),
            "arn:aws:ec2:us-east-1:111122223333:instance/i-abc"
        );
    }

    #[test]
    fn build_honors_non_default_partition() {
        let ctx = ctx_with("aws-cn", "cn-north-1", "999988887777");
        assert_eq!(
            build(&ctx, "s3", "my-bucket"),
            "arn:aws-cn:s3:cn-north-1:999988887777:my-bucket"
        );
    }

    #[test]
    fn build_honors_govcloud_partition() {
        let ctx = ctx_with("aws-us-gov", "us-gov-west-1", "555566667777");
        assert_eq!(
            build(&ctx, "kms", "key/abc-123"),
            "arn:aws-us-gov:kms:us-gov-west-1:555566667777:key/abc-123"
        );
    }

    #[test]
    fn build_global_omits_region() {
        let ctx = ctx_with("aws", "us-east-1", "111122223333");
        assert_eq!(
            build_global(&ctx, "iam", "role/AdminRole"),
            "arn:aws:iam::111122223333:role/AdminRole"
        );
    }

    #[test]
    fn build_partition_omits_region_and_account() {
        let ctx = ctx_with("aws", "us-east-1", "111122223333");
        assert_eq!(
            build_partition(&ctx, "s3", "my-bucket"),
            "arn:aws:s3:::my-bucket"
        );
    }

    #[test]
    fn parse_round_trips_basic_arn() {
        let arn = parse("arn:aws:ec2:us-east-1:111122223333:instance/i-abc").unwrap();
        assert_eq!(arn.partition, "aws");
        assert_eq!(arn.service, "ec2");
        assert_eq!(arn.region, "us-east-1");
        assert_eq!(arn.account, "111122223333");
        assert_eq!(arn.resource, "instance/i-abc");
        assert_eq!(
            arn.to_string(),
            "arn:aws:ec2:us-east-1:111122223333:instance/i-abc"
        );
    }

    #[test]
    fn parse_preserves_colons_in_resource_segment() {
        let raw = "arn:aws:logs:us-east-1:111:log-group:/aws/lambda/foo:log-stream:bar";
        let arn = parse(raw).unwrap();
        assert_eq!(arn.resource, "log-group:/aws/lambda/foo:log-stream:bar");
        assert_eq!(arn.to_string(), raw);
    }

    #[test]
    fn parse_accepts_empty_region_and_account_segments() {
        let arn = parse("arn:aws:iam::111122223333:role/Admin").unwrap();
        assert_eq!(arn.region, "");
        assert_eq!(arn.account, "111122223333");

        let bucket = parse("arn:aws:s3:::my-bucket").unwrap();
        assert_eq!(bucket.region, "");
        assert_eq!(bucket.account, "");
        assert_eq!(bucket.resource, "my-bucket");
    }

    #[test]
    fn parse_rejects_non_arn_string() {
        let err = parse("not-an-arn").unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn parse_rejects_too_few_segments() {
        let err = parse("arn:aws:s3").unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn parse_rejects_empty_partition_or_service() {
        assert!(parse("arn::s3:us-east-1:111:bucket").is_err());
        assert!(parse("arn:aws::us-east-1:111:bucket").is_err());
    }
}
