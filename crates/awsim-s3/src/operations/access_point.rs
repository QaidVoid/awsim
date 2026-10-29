//! S3 Control Access Points: create, describe, list and delete them, and
//! their policies. Data requests made through an Access Point are resolved
//! to its bucket by [`find`].

use awsim_core::{AccountRegionStore, AwsError, RequestContext};
use serde_json::{Value, json};

use super::opt_str;
use crate::error::no_such_bucket;
use crate::state::{AccessPoint, PublicAccessBlock, S3State};
use crate::util::now_iso8601;

/// XML namespace of every S3 Control response.
const NAMESPACE: &str = "http://awss3control.amazonaws.com/doc/2018-08-20/";

/// Suffix AWS gives every Access Point alias.
const ALIAS_SUFFIX: &str = "-s3alias";

/// How much of the name an alias keeps, so the alias stays within the 63
/// characters of a bucket name.
const ALIAS_NAME_PREFIX: usize = 20;

/// Length of the random part of an alias.
const ALIAS_RANDOM_LEN: usize = 34;

/// Page size `ListAccessPoints` uses when none is given, and its maximum.
const MAX_RESULTS: usize = 1000;

fn invalid_access_point(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidAccessPoint", message)
}

fn no_such_access_point(name: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchAccessPoint",
        format!("The specified accesspoint does not exist: {name}"),
    )
}

/// Refuse a name AWS refuses: 3 to 50 lowercase letters, digits and
/// hyphens, starting and ending with a letter or digit, not starting with
/// `xn--` and not ending with `-s3alias`.
pub fn validate_name(name: &str) -> Result<(), AwsError> {
    let valid_chars = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let valid = (3..=50).contains(&name.len())
        && valid_chars
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.starts_with("xn--")
        && !name.ends_with(ALIAS_SUFFIX);
    if valid {
        Ok(())
    } else {
        Err(invalid_access_point(format!(
            "The specified access point name or account is not valid: {name}"
        )))
    }
}

/// The account an S3 Control request names in `x-amz-account-id`.
pub fn account_id(input: &Value) -> Result<&str, AwsError> {
    let account = opt_str(input, "AccountId").unwrap_or_default();
    if account.len() == 12 && account.bytes().all(|b| b.is_ascii_digit()) {
        Ok(account)
    } else {
        Err(invalid_access_point(format!(
            "The specified access point name or account is not valid: account '{account}'"
        )))
    }
}

/// `arn:{partition}:s3:{region}:{account}:accesspoint/{name}`.
pub fn arn(partition: &str, region: &str, account: &str, name: &str) -> String {
    format!("arn:{partition}:s3:{region}:{account}:accesspoint/{name}")
}

fn new_alias(name: &str) -> String {
    let prefix = &name[..name.len().min(ALIAS_NAME_PREFIX)];
    let random: String = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
    .chars()
    .take(ALIAS_RANDOM_LEN)
    .collect();
    format!("{prefix}-{random}{ALIAS_SUFFIX}")
}

fn request_body<'a>(input: &'a Value, root: &str) -> &'a Value {
    input.get(root).unwrap_or(input)
}

fn flag(config: Option<&Value>, field: &str) -> bool {
    config
        .and_then(|c| c.get(field))
        .and_then(Value::as_str)
        .map_or(true, |v| v.eq_ignore_ascii_case("true"))
}

fn key(ctx: &RequestContext, name: &str) -> (String, String) {
    (ctx.region.clone(), name.to_string())
}

/// PUT /v20180820/accesspoint/{Name}
pub fn create_access_point(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let account = account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    validate_name(name)?;
    let body = request_body(input, "CreateAccessPointRequest");
    let bucket = opt_str(body, "Bucket").unwrap_or_default();
    if bucket.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidRequest",
            "Missing required parameter: Bucket",
        ));
    }
    if !state.buckets.contains_key(bucket) {
        return Err(no_such_bucket(bucket));
    }
    let vpc_id = body
        .get("VpcConfiguration")
        .and_then(|v| opt_str(v, "VpcId"))
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let block = body.get("PublicAccessBlockConfiguration");
    let point = AccessPoint {
        name: name.to_string(),
        region: ctx.region.clone(),
        bucket: bucket.to_string(),
        alias: new_alias(name),
        vpc_id,
        public_access_block: PublicAccessBlock {
            block_public_acls: flag(block, "BlockPublicAcls"),
            ignore_public_acls: flag(block, "IgnorePublicAcls"),
            block_public_policy: flag(block, "BlockPublicPolicy"),
            restrict_public_buckets: flag(block, "RestrictPublicBuckets"),
        },
        created_at: now_iso8601(),
        policy: None,
    };
    let alias = point.alias.clone();
    match state.access_points.entry(key(ctx, name)) {
        dashmap::Entry::Occupied(_) => {
            return Err(AwsError::conflict(
                "AccessPointAlreadyOwnedByYou",
                format!("The access point '{name}' already exists and is owned by you"),
            ));
        }
        dashmap::Entry::Vacant(slot) => {
            slot.insert(point);
        }
    }
    Ok(json!({
        "__xml_root": "CreateAccessPointResult",
        "__xml_namespace": NAMESPACE,
        "AccessPointArn": arn(&ctx.partition, &ctx.region, account, name),
        "Alias": alias,
    }))
}

fn network_origin(point: &AccessPoint) -> &'static str {
    if point.vpc_id.is_some() {
        "VPC"
    } else {
        "Internet"
    }
}

fn vpc_configuration(point: &AccessPoint) -> Option<Value> {
    point.vpc_id.as_ref().map(|id| json!({ "VpcId": id }))
}

fn get(state: &S3State, ctx: &RequestContext, name: &str) -> Result<AccessPoint, AwsError> {
    state
        .access_points
        .get(&key(ctx, name))
        .map(|p| p.value().clone())
        .ok_or_else(|| no_such_access_point(name))
}

/// GET /v20180820/accesspoint/{Name}
pub fn get_access_point(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let account = account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    let point = get(state, ctx, name)?;
    let block = point.public_access_block;
    let suffix = format!("{}.amazonaws.com", ctx.region);
    let mut result = json!({
        "__xml_root": "GetAccessPointResult",
        "__xml_namespace": NAMESPACE,
        "Name": point.name,
        "Bucket": point.bucket,
        "NetworkOrigin": network_origin(&point),
        "PublicAccessBlockConfiguration": {
            "BlockPublicAcls": block.block_public_acls,
            "IgnorePublicAcls": block.ignore_public_acls,
            "BlockPublicPolicy": block.block_public_policy,
            "RestrictPublicBuckets": block.restrict_public_buckets,
        },
        "CreationDate": point.created_at,
        "Alias": point.alias,
        "AccessPointArn": arn(&ctx.partition, &ctx.region, account, name),
        "Endpoints": {
            "entry": [
                { "key": "ipv4", "value": format!("s3-accesspoint.{suffix}") },
                { "key": "dualstack", "value": format!("s3-accesspoint.dualstack.{suffix}") },
            ],
        },
        "BucketAccountId": account,
    });
    if let Some(vpc) = vpc_configuration(&point) {
        result["VpcConfiguration"] = vpc;
    }
    Ok(result)
}

/// DELETE /v20180820/accesspoint/{Name}
pub fn delete_access_point(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    state
        .access_points
        .remove(&key(ctx, name))
        .ok_or_else(|| no_such_access_point(name))?;
    Ok(json!({}))
}

/// GET /v20180820/accesspoint
pub fn list_access_points(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let account = account_id(input)?;
    let bucket = opt_str(input, "bucket").filter(|b| !b.is_empty());
    let after = opt_str(input, "nextToken").filter(|t| !t.is_empty());
    let max = match opt_str(input, "maxResults") {
        Some(raw) => match raw.parse::<usize>() {
            Ok(n) if (1..=MAX_RESULTS).contains(&n) => n,
            _ => {
                return Err(AwsError::bad_request(
                    "InvalidRequest",
                    format!("maxResults must be between 1 and {MAX_RESULTS}"),
                ));
            }
        },
        None => MAX_RESULTS,
    };
    let mut points: Vec<AccessPoint> = state
        .access_points
        .iter()
        .filter(|p| p.region == ctx.region)
        .filter(|p| bucket.is_none_or(|b| p.bucket == b))
        .filter(|p| after.is_none_or(|t| p.name.as_str() > t))
        .map(|p| p.value().clone())
        .collect();
    points.sort_by(|a, b| a.name.cmp(&b.name));
    let more = points.len() > max;
    points.truncate(max);
    let entries: Vec<Value> = points
        .iter()
        .map(|point| {
            let mut entry = json!({
                "Name": point.name,
                "NetworkOrigin": network_origin(point),
                "Bucket": point.bucket,
                "AccessPointArn": arn(&ctx.partition, &ctx.region, account, &point.name),
                "Alias": point.alias,
                "BucketAccountId": account,
            });
            if let Some(vpc) = vpc_configuration(point) {
                entry["VpcConfiguration"] = vpc;
            }
            entry
        })
        .collect();
    let mut result = json!({
        "__xml_root": "ListAccessPointsResult",
        "__xml_namespace": NAMESPACE,
        "AccessPointList": { "AccessPoint": entries },
    });
    if more && let Some(last) = points.last() {
        result["NextToken"] = json!(last.name);
    }
    Ok(result)
}

/// PUT /v20180820/accesspoint/{Name}/policy
pub fn put_access_point_policy(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    let policy =
        opt_str(request_body(input, "PutAccessPointPolicyRequest"), "Policy").unwrap_or_default();
    if awsim_iam_policy::parse(policy).is_err() {
        return Err(AwsError::bad_request(
            "MalformedPolicy",
            "Policies must be valid JSON and the first byte must be '{'",
        ));
    }
    let mut point = state
        .access_points
        .get_mut(&key(ctx, name))
        .ok_or_else(|| no_such_access_point(name))?;
    point.policy = Some(policy.to_string());
    Ok(json!({}))
}

/// GET /v20180820/accesspoint/{Name}/policy
pub fn get_access_point_policy(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    let policy = get(state, ctx, name)?.policy.ok_or_else(|| {
        AwsError::not_found(
            "NoSuchAccessPointPolicy",
            format!("The specified accesspoint policy does not exist: {name}"),
        )
    })?;
    Ok(json!({
        "__xml_root": "GetAccessPointPolicyResult",
        "__xml_namespace": NAMESPACE,
        "Policy": policy,
    }))
}

/// DELETE /v20180820/accesspoint/{Name}/policy
pub fn delete_access_point_policy(
    state: &S3State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    account_id(input)?;
    let name = opt_str(input, "Name").unwrap_or_default();
    let mut point = state
        .access_points
        .get_mut(&key(ctx, name))
        .ok_or_else(|| no_such_access_point(name))?;
    point.policy = None;
    Ok(json!({}))
}

/// The path-style path for a request on an Access Point host,
/// `{name}-{account}.{anything}`, as the SDKs and Mountpoint address an
/// Access Point ARN: the original path under the percent-encoded ARN.
///
/// `None` for any other host. No lookup is made, so a request through a
/// deleted Access Point still fails as one.
pub fn path_for_host(host: &str, path: &str, region: &str, partition: &str) -> Option<String> {
    if host.starts_with('[') {
        return None;
    }
    let label = host.split([':', '.']).next()?;
    let (name, account) = label.rsplit_once('-')?;
    let is_account = account.len() == 12 && account.bytes().all(|b| b.is_ascii_digit());
    if !is_account || validate_name(name).is_err() {
        return None;
    }
    let point = arn(partition, region, account, name).replace('/', "%2F");
    let rest = if path == "/" { "" } else { path };
    Some(format!("/{point}{rest}"))
}

/// An Access Point a data request reached, with the account that owns it.
#[derive(Debug, Clone)]
pub struct Reached {
    pub account: String,
    pub arn: String,
    pub point: AccessPoint,
}

/// The Access Point a data request's `Bucket` names by ARN or alias.
///
/// `None` when `bucket` is a plain bucket name, and `NoSuchAccessPoint`
/// when it names an Access Point that does not exist. `partition` builds
/// the ARN of a point reached by alias.
pub fn find(
    store: &AccountRegionStore<S3State>,
    bucket: &str,
    partition: &str,
) -> Option<Result<Reached, AwsError>> {
    if let Some(rest) = bucket.strip_prefix("arn:") {
        let mut parts = rest.splitn(5, ':');
        let (Some(partition), Some("s3"), Some(region), Some(account), Some(resource)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return None;
        };
        let name = resource.strip_prefix("accesspoint/")?;
        let found = store
            .get(account, "global")
            .access_points
            .get(&(region.to_string(), name.to_string()))
            .map(|p| Reached {
                account: account.to_string(),
                arn: arn(partition, region, account, name),
                point: p.value().clone(),
            });
        return Some(found.ok_or_else(|| no_such_access_point(name)));
    }
    if !bucket.ends_with(ALIAS_SUFFIX) {
        return None;
    }
    let found = store
        .iter_all()
        .into_iter()
        .find_map(|((account, _), state)| {
            state
                .access_points
                .iter()
                .find(|p| p.alias == bucket)
                .map(|p| Reached {
                    arn: arn(partition, &p.region, &account, &p.name),
                    account,
                    point: p.value().clone(),
                })
        });
    Some(found.ok_or_else(|| no_such_access_point(bucket)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("s3", "us-east-1")
    }

    fn with_bucket() -> S3State {
        let state = S3State::default();
        crate::operations::bucket::create_bucket(&state, &json!({ "Bucket": "data" }), &ctx())
            .unwrap();
        state
    }

    fn create(state: &S3State, name: &str, body: Value) -> Result<Value, AwsError> {
        create_access_point(
            state,
            &json!({
                "Name": name,
                "AccountId": "000000000000",
                "CreateAccessPointRequest": body,
            }),
            &ctx(),
        )
    }

    fn named(name: &str) -> Value {
        json!({ "Name": name, "AccountId": "000000000000" })
    }

    #[test]
    fn create_returns_arn_and_alias() {
        let state = with_bucket();
        let out = create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        assert_eq!(
            out["AccessPointArn"],
            "arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1"
        );
        let alias = out["Alias"].as_str().unwrap();
        assert!(alias.starts_with("owner-1-") && alias.ends_with("-s3alias"));
        assert!(alias.len() <= 63);
        assert_eq!(out["__xml_namespace"], NAMESPACE);
    }

    #[test]
    fn long_names_keep_the_alias_within_a_bucket_name() {
        let state = with_bucket();
        let name = "a".repeat(50);
        let out = create(&state, &name, json!({ "Bucket": "data" })).unwrap();
        assert_eq!(out["Alias"].as_str().unwrap().len(), 63);
    }

    #[test]
    fn get_reports_vpc_origin_and_public_access_block() {
        let state = with_bucket();
        create(
            &state,
            "owner-1",
            json!({
                "Bucket": "data",
                "VpcConfiguration": { "VpcId": "vpc-1" },
                "PublicAccessBlockConfiguration": { "BlockPublicAcls": "false" },
            }),
        )
        .unwrap();
        let out = get_access_point(&state, &named("owner-1"), &ctx()).unwrap();
        assert_eq!(out["Bucket"], "data");
        assert_eq!(out["NetworkOrigin"], "VPC");
        assert_eq!(out["VpcConfiguration"]["VpcId"], "vpc-1");
        assert_eq!(
            out["PublicAccessBlockConfiguration"]["BlockPublicAcls"],
            false
        );
        assert_eq!(
            out["PublicAccessBlockConfiguration"]["IgnorePublicAcls"],
            true
        );
        assert_eq!(out["BucketAccountId"], "000000000000");
        assert!(out["CreationDate"].as_str().is_some());
    }

    #[test]
    fn internet_origin_has_no_vpc_configuration() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let out = get_access_point(&state, &named("owner-1"), &ctx()).unwrap();
        assert_eq!(out["NetworkOrigin"], "Internet");
        assert!(out.get("VpcConfiguration").is_none());
    }

    #[test]
    fn duplicate_name_is_already_owned() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let err = create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap_err();
        assert_eq!(err.code, "AccessPointAlreadyOwnedByYou");
        assert_eq!(err.status.as_u16(), 409);
    }

    #[test]
    fn same_name_in_another_region_is_separate() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let west = RequestContext::new("s3", "us-west-2");
        let input = json!({
            "Name": "owner-1",
            "AccountId": "000000000000",
            "CreateAccessPointRequest": { "Bucket": "data" },
        });
        create_access_point(&state, &input, &west).unwrap();
        assert_eq!(state.access_points.len(), 2);
    }

    #[test]
    fn missing_bucket_creates_nothing() {
        let state = with_bucket();
        let err = create(&state, "owner-1", json!({ "Bucket": "absent" })).unwrap_err();
        assert_eq!(err.code, "NoSuchBucket");
        assert!(state.access_points.is_empty());
    }

    #[test]
    fn invalid_names_are_refused() {
        let state = with_bucket();
        for name in [
            "ab",
            &"a".repeat(51),
            "Upper",
            "under_score",
            "dot.ted",
            "-lead",
            "trail-",
            "xn--punycode",
            "name-s3alias",
        ] {
            let err = create(&state, name, json!({ "Bucket": "data" })).unwrap_err();
            assert_eq!(err.code, "InvalidAccessPoint", "{name}");
            assert_eq!(err.status.as_u16(), 400);
        }
    }

    #[test]
    fn invalid_account_is_refused() {
        let state = with_bucket();
        let input = json!({
            "Name": "owner-1",
            "AccountId": "12",
            "CreateAccessPointRequest": { "Bucket": "data" },
        });
        let err = create_access_point(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidAccessPoint");
    }

    #[test]
    fn missing_point_is_no_such_access_point() {
        let state = with_bucket();
        for err in [
            get_access_point(&state, &named("absent"), &ctx()).unwrap_err(),
            delete_access_point(&state, &named("absent"), &ctx()).unwrap_err(),
            get_access_point_policy(&state, &named("absent"), &ctx()).unwrap_err(),
            delete_access_point_policy(&state, &named("absent"), &ctx()).unwrap_err(),
        ] {
            assert_eq!(err.code, "NoSuchAccessPoint");
            assert_eq!(err.status.as_u16(), 404);
        }
    }

    #[test]
    fn delete_removes_the_point() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        delete_access_point(&state, &named("owner-1"), &ctx()).unwrap();
        let err = get_access_point(&state, &named("owner-1"), &ctx()).unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPoint");
    }

    #[test]
    fn list_filters_by_bucket_and_pages_in_name_order() {
        let state = with_bucket();
        crate::operations::bucket::create_bucket(&state, &json!({ "Bucket": "other" }), &ctx())
            .unwrap();
        for name in ["ccc", "aaa", "bbb"] {
            create(&state, name, json!({ "Bucket": "data" })).unwrap();
        }
        create(&state, "zzz", json!({ "Bucket": "other" })).unwrap();

        let page = |token: Option<&str>| {
            let mut input =
                json!({ "AccountId": "000000000000", "bucket": "data", "maxResults": "2" });
            if let Some(t) = token {
                input["nextToken"] = json!(t);
            }
            list_access_points(&state, &input, &ctx()).unwrap()
        };
        let first = page(None);
        let names: Vec<&str> = first["AccessPointList"]["AccessPoint"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["Name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["aaa", "bbb"]);
        let token = first["NextToken"].as_str().unwrap();
        let second = page(Some(token));
        assert_eq!(second["AccessPointList"]["AccessPoint"][0]["Name"], "ccc");
        assert!(second.get("NextToken").is_none());

        let all =
            list_access_points(&state, &json!({ "AccountId": "000000000000" }), &ctx()).unwrap();
        assert_eq!(
            all["AccessPointList"]["AccessPoint"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn list_refuses_an_out_of_range_page_size() {
        let state = with_bucket();
        let input = json!({ "AccountId": "000000000000", "maxResults": "0" });
        let err = list_access_points(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidRequest");
    }

    #[test]
    fn policy_round_trips_and_deletes() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let err = get_access_point_policy(&state, &named("owner-1"), &ctx()).unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPointPolicy");
        assert_eq!(err.status.as_u16(), 404);

        let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"*"},"Action":"s3:GetObject","Resource":"arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1/object/*"}]}"#;
        let mut input = named("owner-1");
        input["PutAccessPointPolicyRequest"] = json!({ "Policy": policy });
        put_access_point_policy(&state, &input, &ctx()).unwrap();
        let out = get_access_point_policy(&state, &named("owner-1"), &ctx()).unwrap();
        assert_eq!(out["Policy"], policy);

        delete_access_point_policy(&state, &named("owner-1"), &ctx()).unwrap();
        let err = get_access_point_policy(&state, &named("owner-1"), &ctx()).unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPointPolicy");
    }

    #[test]
    fn malformed_policy_is_refused() {
        let state = with_bucket();
        create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        for policy in ["not json", "[]", ""] {
            let mut input = named("owner-1");
            input["PutAccessPointPolicyRequest"] = json!({ "Policy": policy });
            let err = put_access_point_policy(&state, &input, &ctx()).unwrap_err();
            assert_eq!(err.code, "MalformedPolicy", "{policy}");
        }
    }

    #[test]
    fn find_resolves_arn_and_alias_and_ignores_buckets() {
        let store: AccountRegionStore<S3State> = AccountRegionStore::new();
        let state = store.get("000000000000", "global");
        crate::operations::bucket::create_bucket(&state, &json!({ "Bucket": "data" }), &ctx())
            .unwrap();
        let out = create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let alias = out["Alias"].as_str().unwrap();

        let by_arn = find(
            &store,
            "arn:aws:s3:us-east-1:000000000000:accesspoint/owner-1",
            "aws",
        )
        .unwrap()
        .unwrap();
        assert_eq!(by_arn.point.bucket, "data");
        assert_eq!(by_arn.account, "000000000000");
        let by_alias = find(&store, alias, "aws").unwrap().unwrap();
        assert_eq!(by_alias.arn, by_arn.arn);

        assert!(find(&store, "data", "aws").is_none());
        let err = find(
            &store,
            "arn:aws:s3:us-west-2:000000000000:accesspoint/owner-1",
            "aws",
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPoint");
        let err = find(&store, "gone-abc-s3alias", "aws")
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, "NoSuchAccessPoint");
    }

    #[test]
    fn access_point_hosts_route_under_the_arn() {
        let arn = "/arn:aws:s3:us-east-1:000000000000:accesspoint%2Fowner-1";
        for host in [
            "owner-1-000000000000.aws.qaidvoid.dev:4567",
            "owner-1-000000000000.localhost:4566",
            "owner-1-000000000000.s3-accesspoint.us-east-1.amazonaws.com",
        ] {
            assert_eq!(
                path_for_host(host, "/", "us-east-1", "aws").as_deref(),
                Some(arn),
                "{host}"
            );
            assert_eq!(
                path_for_host(host, "/dir/a%20b", "us-east-1", "aws"),
                Some(format!("{arn}/dir/a%20b")),
                "{host}"
            );
        }
    }

    #[test]
    fn other_hosts_keep_their_path() {
        for host in [
            "localhost:4566",
            "aws.qaidvoid.dev:4567",
            "000000000000.aws.qaidvoid.dev:4567",
            "s3.us-east-1.amazonaws.com",
            "my-bucket.s3.amazonaws.com",
            "Bad_Name-000000000000.localhost",
            "owner-1-00000000000.localhost",
            "127.0.0.1:4566",
            "[::1]:4566",
        ] {
            assert_eq!(
                path_for_host(host, "/k", "us-east-1", "aws"),
                None,
                "{host}"
            );
        }
    }

    #[test]
    fn access_points_survive_a_snapshot() {
        use awsim_core::Snapshottable;
        let state = with_bucket();
        let out = create(&state, "owner-1", json!({ "Bucket": "data" })).unwrap();
        let mut input = named("owner-1");
        input["PutAccessPointPolicyRequest"] = json!({ "Policy": r#"{"Statement":[]}"# });
        put_access_point_policy(&state, &input, &ctx()).unwrap();

        let snapshot = state.to_snapshot("000000000000", "global");
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        let (_, _, restored) = S3State::from_snapshot(serde_json::from_slice(&bytes).unwrap());
        let point = get(&restored, &ctx(), "owner-1").unwrap();
        assert_eq!(point.alias, out["Alias"].as_str().unwrap());
        assert_eq!(point.policy.as_deref(), Some(r#"{"Statement":[]}"#));
    }
}
