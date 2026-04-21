use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

/// Hardcoded list of AWS regions.
pub fn describe_regions(_ctx: &RequestContext) -> Result<Value, AwsError> {
    let regions = vec![
        "us-east-1",
        "us-east-2",
        "us-west-1",
        "us-west-2",
        "eu-west-1",
        "eu-west-2",
        "eu-west-3",
        "eu-central-1",
        "eu-north-1",
        "eu-south-1",
        "ap-southeast-1",
        "ap-southeast-2",
        "ap-northeast-1",
        "ap-northeast-2",
        "ap-northeast-3",
        "ap-south-1",
        "ap-east-1",
        "ca-central-1",
        "sa-east-1",
        "me-south-1",
        "af-south-1",
    ];

    let items: Vec<Value> = regions
        .iter()
        .map(|r| {
            json!({
                "regionName": r,
                "regionEndpoint": format!("ec2.{r}.amazonaws.com"),
                "optInStatus": "opt-in-not-required",
            })
        })
        .collect();

    Ok(json!({ "regionInfo": { "item": items } }))
}

/// Return availability zones for the current region.
pub fn describe_availability_zones(ctx: &RequestContext) -> Result<Value, AwsError> {
    let region = &ctx.region;

    // Most regions have 3 AZs (a/b/c), some have 2 or more.
    let zone_suffixes = ["a", "b", "c"];

    let items: Vec<Value> = zone_suffixes
        .iter()
        .map(|suffix| {
            let zone_name = format!("{region}{suffix}");
            json!({
                "zoneName": zone_name,
                "zoneId": format!("{region}-az{suffix}"),
                "state": "available",
                "regionName": region,
            })
        })
        .collect();

    Ok(json!({ "availabilityZoneInfo": { "item": items } }))
}
