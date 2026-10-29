use awsim_core::{AccountRegionStore, RequestContext, ResourcePolicyLookup, arn};
use awsim_iam_policy::PolicyDocument;
use serde_json::Value;

use crate::state::S3State;

pub struct S3ResourcePolicyLookup {
    store: AccountRegionStore<S3State>,
}

impl S3ResourcePolicyLookup {
    pub fn new(store: AccountRegionStore<S3State>) -> Self {
        Self { store }
    }
}

fn extract_bucket(arn: &str) -> Option<&str> {
    let rest = arn.strip_prefix("arn:aws:s3:::")?;
    let bucket = rest.split('/').next()?;
    if bucket.is_empty() {
        None
    } else {
        Some(bucket)
    }
}

/// The `(account, region, name)` of an Access Point resource ARN,
/// `arn:{partition}:s3:{region}:{account}:accesspoint/{name}[/object/{key}]`.
fn extract_access_point(arn: &str) -> Option<(&str, &str, &str)> {
    let mut parts = arn.splitn(6, ':');
    let (Some("arn"), Some(_), Some("s3"), Some(region), Some(account), Some(resource)) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return None;
    };
    let name = resource.strip_prefix("accesspoint/")?.split('/').next()?;
    Some((account, region, name))
}

impl ResourcePolicyLookup for S3ResourcePolicyLookup {
    fn lookup(&self, resource_arn: &str) -> Option<PolicyDocument> {
        if let Some((account, region, name)) = extract_access_point(resource_arn) {
            let state = self.store.get(account, "global");
            let point = state
                .access_points
                .get(&(region.to_string(), name.to_string()))?;
            return awsim_iam_policy::parse(point.policy.as_deref()?).ok();
        }
        let bucket_name = extract_bucket(resource_arn)?;
        for (_, state) in self.store.iter_all() {
            if let Some(bucket) = state.buckets.get(bucket_name)
                && let Some(policy) = bucket.policy.as_deref()
            {
                return awsim_iam_policy::parse(policy).ok();
            }
        }
        None
    }
}

/// IAM action for a bucket-level operation, per the S3 service
/// authorization reference. Several operations share one action
/// (DeleteBucketCors needs `s3:PutBucketCORS`).
fn bucket_action(operation: &str) -> Option<&'static str> {
    Some(match operation {
        "CreateBucket" => "s3:CreateBucket",
        "DeleteBucket" => "s3:DeleteBucket",
        "HeadBucket" | "ListObjects" | "ListObjectsV2" => "s3:ListBucket",
        "ListObjectVersions" => "s3:ListBucketVersions",
        "ListMultipartUploads" => "s3:ListBucketMultipartUploads",
        "GetBucketLocation" => "s3:GetBucketLocation",
        "GetBucketTagging" => "s3:GetBucketTagging",
        "PutBucketTagging" | "DeleteBucketTagging" => "s3:PutBucketTagging",
        "GetBucketVersioning" => "s3:GetBucketVersioning",
        "PutBucketVersioning" => "s3:PutBucketVersioning",
        "GetBucketPolicy" => "s3:GetBucketPolicy",
        "PutBucketPolicy" => "s3:PutBucketPolicy",
        "DeleteBucketPolicy" => "s3:DeleteBucketPolicy",
        "GetBucketPolicyStatus" => "s3:GetBucketPolicyStatus",
        "GetBucketCors" => "s3:GetBucketCORS",
        "PutBucketCors" | "DeleteBucketCors" => "s3:PutBucketCORS",
        "GetBucketNotificationConfiguration" => "s3:GetBucketNotification",
        "PutBucketNotificationConfiguration" => "s3:PutBucketNotification",
        "GetBucketAcl" => "s3:GetBucketAcl",
        "PutBucketAcl" => "s3:PutBucketAcl",
        "GetBucketLifecycleConfiguration" => "s3:GetLifecycleConfiguration",
        "PutBucketLifecycleConfiguration" | "DeleteBucketLifecycleConfiguration" => {
            "s3:PutLifecycleConfiguration"
        }
        "GetBucketEncryption" => "s3:GetEncryptionConfiguration",
        "PutBucketEncryption" | "DeleteBucketEncryption" => "s3:PutEncryptionConfiguration",
        "GetBucketLogging" => "s3:GetBucketLogging",
        "PutBucketLogging" => "s3:PutBucketLogging",
        "GetBucketWebsite" => "s3:GetBucketWebsite",
        "PutBucketWebsite" => "s3:PutBucketWebsite",
        "DeleteBucketWebsite" => "s3:DeleteBucketWebsite",
        "GetBucketReplication" => "s3:GetReplicationConfiguration",
        "PutBucketReplication" | "DeleteBucketReplication" => "s3:PutReplicationConfiguration",
        "GetBucketRequestPayment" => "s3:GetBucketRequestPayment",
        "PutBucketRequestPayment" => "s3:PutBucketRequestPayment",
        "GetBucketAccelerateConfiguration" => "s3:GetAccelerateConfiguration",
        "PutBucketAccelerateConfiguration" => "s3:PutAccelerateConfiguration",
        "GetBucketAnalyticsConfiguration" | "ListBucketAnalyticsConfigurations" => {
            "s3:GetAnalyticsConfiguration"
        }
        "PutBucketAnalyticsConfiguration" | "DeleteBucketAnalyticsConfiguration" => {
            "s3:PutAnalyticsConfiguration"
        }
        "GetBucketMetricsConfiguration" | "ListBucketMetricsConfigurations" => {
            "s3:GetMetricsConfiguration"
        }
        "PutBucketMetricsConfiguration" | "DeleteBucketMetricsConfiguration" => {
            "s3:PutMetricsConfiguration"
        }
        "GetBucketIntelligentTieringConfiguration"
        | "ListBucketIntelligentTieringConfigurations" => "s3:GetIntelligentTieringConfiguration",
        "PutBucketIntelligentTieringConfiguration"
        | "DeleteBucketIntelligentTieringConfiguration" => "s3:PutIntelligentTieringConfiguration",
        "GetBucketInventoryConfiguration" | "ListBucketInventoryConfigurations" => {
            "s3:GetInventoryConfiguration"
        }
        "PutBucketInventoryConfiguration" | "DeleteBucketInventoryConfiguration" => {
            "s3:PutInventoryConfiguration"
        }
        "GetBucketOwnershipControls" => "s3:GetBucketOwnershipControls",
        "PutBucketOwnershipControls" | "DeleteBucketOwnershipControls" => {
            "s3:PutBucketOwnershipControls"
        }
        "GetPublicAccessBlock" => "s3:GetBucketPublicAccessBlock",
        "PutPublicAccessBlock" | "DeletePublicAccessBlock" => "s3:PutBucketPublicAccessBlock",
        "GetObjectLockConfiguration" => "s3:GetBucketObjectLockConfiguration",
        "PutObjectLockConfiguration" => "s3:PutBucketObjectLockConfiguration",
        _ => return None,
    })
}

/// IAM action for an S3 Control Access Point operation, which shares its
/// name with the operation.
fn control_action(operation: &str) -> Option<&'static str> {
    Some(match operation {
        "CreateAccessPoint" => "s3:CreateAccessPoint",
        "GetAccessPoint" => "s3:GetAccessPoint",
        "DeleteAccessPoint" => "s3:DeleteAccessPoint",
        "ListAccessPoints" => "s3:ListAccessPoints",
        "PutAccessPointPolicy" => "s3:PutAccessPointPolicy",
        "GetAccessPointPolicy" => "s3:GetAccessPointPolicy",
        "DeleteAccessPointPolicy" => "s3:DeleteAccessPointPolicy",
        _ => return None,
    })
}

/// IAM action for an object-level operation as `(plain, versioned)`:
/// the second applies when the request names a `versionId`.
fn object_action(operation: &str) -> Option<(&'static str, &'static str)> {
    Some(match operation {
        "GetObject" | "HeadObject" | "SelectObjectContent" => {
            ("s3:GetObject", "s3:GetObjectVersion")
        }
        "PutObject"
        | "CopyObject"
        | "CreateMultipartUpload"
        | "UploadPart"
        | "UploadPartCopy"
        | "CompleteMultipartUpload" => ("s3:PutObject", "s3:PutObject"),
        "DeleteObject" => ("s3:DeleteObject", "s3:DeleteObjectVersion"),
        "AbortMultipartUpload" => ("s3:AbortMultipartUpload", "s3:AbortMultipartUpload"),
        "ListParts" => ("s3:ListMultipartUploadParts", "s3:ListMultipartUploadParts"),
        "GetObjectAcl" => ("s3:GetObjectAcl", "s3:GetObjectVersionAcl"),
        "PutObjectAcl" => ("s3:PutObjectAcl", "s3:PutObjectVersionAcl"),
        "GetObjectTagging" => ("s3:GetObjectTagging", "s3:GetObjectVersionTagging"),
        "PutObjectTagging" => ("s3:PutObjectTagging", "s3:PutObjectVersionTagging"),
        "DeleteObjectTagging" => ("s3:DeleteObjectTagging", "s3:DeleteObjectVersionTagging"),
        "GetObjectLegalHold" => ("s3:GetObjectLegalHold", "s3:GetObjectLegalHold"),
        "PutObjectLegalHold" => ("s3:PutObjectLegalHold", "s3:PutObjectLegalHold"),
        "GetObjectRetention" => ("s3:GetObjectRetention", "s3:GetObjectRetention"),
        "PutObjectRetention" => ("s3:PutObjectRetention", "s3:PutObjectRetention"),
        "RestoreObject" => ("s3:RestoreObject", "s3:RestoreObject"),
        _ => return None,
    })
}

fn has_version_id(value: &Value) -> bool {
    value
        .get("VersionId")
        .or_else(|| value.get("versionId"))
        .and_then(Value::as_str)
        .is_some_and(|v| !v.is_empty())
}

/// The read check on an `x-amz-copy-source` (`bucket/key[?versionId=v]`).
fn copy_source_check(copy_source: &str, ctx: &RequestContext) -> Option<(String, String)> {
    let source = copy_source.trim_start_matches('/');
    let (path, query) = source.split_once('?').unwrap_or((source, ""));
    if !path.contains('/') {
        return None;
    }
    let versioned = query.split('&').any(|kv| kv.starts_with("versionId="));
    let action = if versioned {
        "s3:GetObjectVersion"
    } else {
        "s3:GetObject"
    };
    Some((action.to_string(), arn::build_partition(ctx, "s3", path)))
}

/// The condition keys AWS adds to a listing's `s3:ListBucket` (or `s3:ListBucketVersions`)
/// check from its query: `s3:prefix`, `s3:delimiter` and `s3:max-keys`, each only when the
/// request names it, so a policy allowing a listing under some prefixes can be met.
pub(crate) fn listing_context(
    operation: &str,
    input: &Value,
) -> std::collections::HashMap<String, awsim_core::ContextValue> {
    use awsim_core::ContextValue;
    let mut context = std::collections::HashMap::new();
    if !matches!(
        operation,
        "ListObjects" | "ListObjectsV2" | "ListObjectVersions"
    ) {
        return context;
    }
    for (param, key) in [("prefix", "s3:prefix"), ("delimiter", "s3:delimiter")] {
        if let Some(value) = input.get(param).and_then(Value::as_str) {
            context.insert(key.to_string(), ContextValue::String(value.to_string()));
        }
    }
    let max_keys = input.get("max-keys").and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });
    if let Some(max_keys) = max_keys {
        context.insert("s3:max-keys".to_string(), ContextValue::Number(max_keys));
    }
    context
}

/// The checks for a request made through an Access Point, from the
/// checks the same request makes on its bucket.
///
/// Each check on the bucket is made twice, as AWS makes it: on the Access
/// Point resource, where the identity and Access Point policies apply, and
/// on the bucket, where the bucket policy applies. Both carry the Access
/// Point's condition keys. A check on another bucket, a copy source say,
/// is kept as it is.
pub(crate) fn through_access_point(
    checks: Vec<awsim_core::IamCheck>,
    reached: &crate::operations::access_point::Reached,
    ctx: &RequestContext,
) -> Vec<awsim_core::IamCheck> {
    use awsim_core::ContextValue;
    let bucket_arn = arn::build_partition(ctx, "s3", &reached.point.bucket);
    let origin = if reached.point.vpc_id.is_some() {
        "VPC"
    } else {
        "Internet"
    };
    let keys = [
        ("s3:DataAccessPointArn", reached.arn.clone()),
        ("s3:DataAccessPointAccount", reached.account.clone()),
        ("s3:AccessPointNetworkOrigin", origin.to_string()),
    ];
    let mut out = Vec::with_capacity(checks.len() * 2);
    for mut check in checks {
        let point_resource = if check.resource == bucket_arn {
            reached.arn.clone()
        } else if let Some(key) = check.resource.strip_prefix(&format!("{bucket_arn}/")) {
            format!("{}/object/{key}", reached.arn)
        } else {
            out.push(check);
            continue;
        };
        for (key, value) in &keys {
            check
                .context
                .insert(key.to_string(), ContextValue::String(value.clone()));
        }
        let mut on_point = check.clone();
        on_point.resource = point_resource;
        out.push(on_point);
        out.push(check);
    }
    out
}

/// Every `(action, resource)` pair IAM must allow for an S3 request,
/// per the S3 service authorization reference. Empty for operations
/// awsim does not enforce.
pub(crate) fn iam_checks(
    operation: &str,
    input: &Value,
    ctx: &RequestContext,
) -> Vec<(String, String)> {
    if operation == "ListBuckets" {
        return vec![("s3:ListAllMyBuckets".to_string(), "*".to_string())];
    }
    if let Some(action) = control_action(operation) {
        if operation == "ListAccessPoints" {
            return vec![(action.to_string(), "*".to_string())];
        }
        let account = input.get("AccountId").and_then(Value::as_str).unwrap_or("");
        let name = input.get("Name").and_then(Value::as_str).unwrap_or("");
        let resource =
            crate::operations::access_point::arn(&ctx.partition, &ctx.region, account, name);
        return vec![(action.to_string(), resource)];
    }
    let Some(bucket) = input.get("Bucket").and_then(Value::as_str) else {
        return Vec::new();
    };
    let object_arn = |key: &str| arn::build_partition(ctx, "s3", format!("{bucket}/{key}"));
    let key = input.get("Key").and_then(Value::as_str).unwrap_or("");

    if let Some(action) = bucket_action(operation) {
        return vec![(action.to_string(), arn::build_partition(ctx, "s3", bucket))];
    }

    match operation {
        // Directory buckets authorize object calls through a session.
        "CreateSession" | "RenameObject" => {
            return vec![(
                "s3express:CreateSession".to_string(),
                arn::build(ctx, "s3express", format!("bucket/{bucket}")),
            )];
        }
        "DeleteObjects" => {
            let objects = input
                .get("Delete")
                .and_then(|d| d.get("Object"))
                .or_else(|| input.get("Object"));
            let objects: Vec<&Value> = match objects {
                Some(Value::Array(items)) => items.iter().collect(),
                Some(item @ Value::Object(_)) => vec![item],
                _ => Vec::new(),
            };
            return objects
                .into_iter()
                .filter_map(|item| {
                    let key = item.get("Key").and_then(Value::as_str)?;
                    let action = if has_version_id(item) {
                        "s3:DeleteObjectVersion"
                    } else {
                        "s3:DeleteObject"
                    };
                    Some((action.to_string(), object_arn(key)))
                })
                .collect();
        }
        "GetObjectAttributes" => {
            let actions = if has_version_id(input) {
                ["s3:GetObjectVersion", "s3:GetObjectVersionAttributes"]
            } else {
                ["s3:GetObject", "s3:GetObjectAttributes"]
            };
            return actions
                .into_iter()
                .map(|action| (action.to_string(), object_arn(key)))
                .collect();
        }
        _ => {}
    }

    let Some((plain, versioned)) = object_action(operation) else {
        return Vec::new();
    };
    // PutObject and UploadPart carrying x-amz-copy-source are CopyObject
    // and UploadPartCopy: read the source, write the target.
    let copy_source = input.get("CopySource").and_then(Value::as_str);
    let action = if has_version_id(input) && copy_source.is_none() {
        versioned
    } else {
        plain
    };
    let mut checks = vec![(action.to_string(), object_arn(key))];
    if let Some(source) = copy_source.and_then(|s| copy_source_check(s, ctx)) {
        checks.insert(0, source);
    }
    checks
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn checks(operation: &str, input: Value) -> Vec<(String, String)> {
        iam_checks(operation, &input, &RequestContext::new("s3", "us-east-1"))
    }

    fn pair(action: &str, resource: &str) -> (String, String) {
        (action.to_string(), resource.to_string())
    }

    #[test]
    fn test_iam_checks_bucket_operations_use_iam_action_names() {
        let bucket = json!({ "Bucket": "b" });
        for (operation, action) in [
            ("ListObjects", "s3:ListBucket"),
            ("ListObjectsV2", "s3:ListBucket"),
            ("HeadBucket", "s3:ListBucket"),
            ("ListObjectVersions", "s3:ListBucketVersions"),
            ("ListMultipartUploads", "s3:ListBucketMultipartUploads"),
            ("GetBucketCors", "s3:GetBucketCORS"),
            ("PutBucketCors", "s3:PutBucketCORS"),
            ("DeleteBucketCors", "s3:PutBucketCORS"),
            ("DeleteBucketTagging", "s3:PutBucketTagging"),
            ("GetBucketEncryption", "s3:GetEncryptionConfiguration"),
            (
                "GetBucketLifecycleConfiguration",
                "s3:GetLifecycleConfiguration",
            ),
            (
                "GetBucketNotificationConfiguration",
                "s3:GetBucketNotification",
            ),
            ("GetPublicAccessBlock", "s3:GetBucketPublicAccessBlock"),
            (
                "GetObjectLockConfiguration",
                "s3:GetBucketObjectLockConfiguration",
            ),
            ("GetBucketReplication", "s3:GetReplicationConfiguration"),
        ] {
            assert_eq!(
                checks(operation, bucket.clone()),
                vec![pair(action, "arn:aws:s3:::b")],
                "{operation}"
            );
        }
        assert_eq!(
            checks("ListBuckets", json!({})),
            vec![pair("s3:ListAllMyBuckets", "*")]
        );
    }

    #[test]
    fn test_iam_checks_access_point_operations_use_access_point_arn() {
        let point = json!({ "Name": "ap", "AccountId": "111111111111" });
        for (operation, action) in [
            ("CreateAccessPoint", "s3:CreateAccessPoint"),
            ("GetAccessPoint", "s3:GetAccessPoint"),
            ("DeleteAccessPoint", "s3:DeleteAccessPoint"),
            ("PutAccessPointPolicy", "s3:PutAccessPointPolicy"),
            ("GetAccessPointPolicy", "s3:GetAccessPointPolicy"),
            ("DeleteAccessPointPolicy", "s3:DeleteAccessPointPolicy"),
        ] {
            assert_eq!(
                checks(operation, point.clone()),
                vec![pair(
                    action,
                    "arn:aws:s3:us-east-1:111111111111:accesspoint/ap"
                )],
                "{operation}"
            );
        }
        assert_eq!(
            checks("ListAccessPoints", json!({ "AccountId": "111111111111" })),
            vec![pair("s3:ListAccessPoints", "*")]
        );
    }

    fn reached(vpc: Option<&str>) -> crate::operations::access_point::Reached {
        crate::operations::access_point::Reached {
            account: "000000000000".to_string(),
            arn: "arn:aws:s3:us-east-1:000000000000:accesspoint/ap".to_string(),
            point: crate::state::AccessPoint {
                name: "ap".to_string(),
                region: "us-east-1".to_string(),
                bucket: "b".to_string(),
                alias: "ap-x-s3alias".to_string(),
                vpc_id: vpc.map(str::to_string),
                public_access_block: crate::state::PublicAccessBlock {
                    block_public_acls: true,
                    ignore_public_acls: true,
                    block_public_policy: true,
                    restrict_public_buckets: true,
                },
                created_at: String::new(),
                policy: None,
            },
        }
    }

    #[test]
    fn test_access_point_checks_cover_the_point_and_the_bucket() {
        use awsim_core::{ContextValue, IamCheck};
        let ctx = RequestContext::new("s3", "us-east-1");
        let checks = vec![
            IamCheck::new("s3:GetObject", "arn:aws:s3:::src/k"),
            IamCheck::new("s3:PutObject", "arn:aws:s3:::b/dir/k"),
            IamCheck::new("s3:ListBucket", "arn:aws:s3:::b"),
        ];
        let out = through_access_point(checks, &reached(Some("vpc-1")), &ctx);
        let pairs: Vec<(&str, &str)> = out
            .iter()
            .map(|c| (c.action.as_str(), c.resource.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("s3:GetObject", "arn:aws:s3:::src/k"),
                (
                    "s3:PutObject",
                    "arn:aws:s3:us-east-1:000000000000:accesspoint/ap/object/dir/k"
                ),
                ("s3:PutObject", "arn:aws:s3:::b/dir/k"),
                (
                    "s3:ListBucket",
                    "arn:aws:s3:us-east-1:000000000000:accesspoint/ap"
                ),
                ("s3:ListBucket", "arn:aws:s3:::b"),
            ]
        );
        assert!(out[0].context.is_empty(), "another bucket gets no keys");
        for check in &out[1..] {
            assert_eq!(
                check.context.get("s3:DataAccessPointArn"),
                Some(&ContextValue::String(
                    "arn:aws:s3:us-east-1:000000000000:accesspoint/ap".to_string()
                ))
            );
            assert_eq!(
                check.context.get("s3:DataAccessPointAccount"),
                Some(&ContextValue::String("000000000000".to_string()))
            );
            assert_eq!(
                check.context.get("s3:AccessPointNetworkOrigin"),
                Some(&ContextValue::String("VPC".to_string()))
            );
        }
    }

    #[test]
    fn test_resource_policy_lookup_reads_access_point_policies() {
        let store: AccountRegionStore<S3State> = AccountRegionStore::new();
        let mut point = reached(None).point;
        point.policy = Some(r#"{"Version":"2012-10-17","Statement":[]}"#.to_string());
        store
            .get("000000000000", "global")
            .access_points
            .insert(("us-east-1".to_string(), "ap".to_string()), point);
        let lookup = S3ResourcePolicyLookup::new(store);
        let arn = "arn:aws:s3:us-east-1:000000000000:accesspoint/ap";
        assert!(lookup.lookup(arn).is_some());
        assert!(lookup.lookup(&format!("{arn}/object/k")).is_some());
        assert!(
            lookup
                .lookup("arn:aws:s3:us-west-2:000000000000:accesspoint/ap")
                .is_none()
        );
    }

    #[test]
    fn test_iam_checks_object_operations_use_object_arn() {
        let object = json!({ "Bucket": "b", "Key": "dir/k" });
        for (operation, action) in [
            ("GetObject", "s3:GetObject"),
            ("HeadObject", "s3:GetObject"),
            ("PutObject", "s3:PutObject"),
            ("CreateMultipartUpload", "s3:PutObject"),
            ("UploadPart", "s3:PutObject"),
            ("CompleteMultipartUpload", "s3:PutObject"),
            ("AbortMultipartUpload", "s3:AbortMultipartUpload"),
            ("ListParts", "s3:ListMultipartUploadParts"),
            ("DeleteObject", "s3:DeleteObject"),
        ] {
            assert_eq!(
                checks(operation, object.clone()),
                vec![pair(action, "arn:aws:s3:::b/dir/k")],
                "{operation}"
            );
        }
    }

    #[test]
    fn test_iam_checks_version_id_selects_version_actions() {
        let object = json!({ "Bucket": "b", "Key": "k", "versionId": "v1" });
        for (operation, action) in [
            ("GetObject", "s3:GetObjectVersion"),
            ("HeadObject", "s3:GetObjectVersion"),
            ("DeleteObject", "s3:DeleteObjectVersion"),
            ("GetObjectTagging", "s3:GetObjectVersionTagging"),
            ("GetObjectAcl", "s3:GetObjectVersionAcl"),
        ] {
            assert_eq!(
                checks(operation, object.clone()),
                vec![pair(action, "arn:aws:s3:::b/k")],
                "{operation}"
            );
        }
    }

    #[test]
    fn test_iam_checks_copy_reads_source_and_writes_target() {
        assert_eq!(
            checks(
                "PutObject",
                json!({ "Bucket": "dst", "Key": "to", "CopySource": "/src/from?versionId=v" })
            ),
            vec![
                pair("s3:GetObjectVersion", "arn:aws:s3:::src/from"),
                pair("s3:PutObject", "arn:aws:s3:::dst/to"),
            ]
        );
        assert_eq!(
            checks(
                "UploadPart",
                json!({ "Bucket": "dst", "Key": "to", "CopySource": "src/from" })
            ),
            vec![
                pair("s3:GetObject", "arn:aws:s3:::src/from"),
                pair("s3:PutObject", "arn:aws:s3:::dst/to"),
            ]
        );
    }

    #[test]
    fn test_iam_checks_delete_objects_checks_every_key() {
        let input = json!({ "Bucket": "b", "Delete": { "Object": [
            { "Key": "a" },
            { "Key": "c", "VersionId": "v" },
        ] } });
        assert_eq!(
            checks("DeleteObjects", input),
            vec![
                pair("s3:DeleteObject", "arn:aws:s3:::b/a"),
                pair("s3:DeleteObjectVersion", "arn:aws:s3:::b/c"),
            ]
        );
    }
}
