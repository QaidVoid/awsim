use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{DelegationSet, Route53State, TrafficPolicy, VpcAssociation};

fn resolve_zone_id(id_raw: &str) -> String {
    if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let dy = if leap { 366 } else { 365 };
        if remaining < dy {
            break;
        }
        remaining -= dy;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mds: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for &md in mds {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    let (mo, d) = (month + 1, remaining + 1);
    format!("{year:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

pub fn get_change(
    _state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let normalized = if id.starts_with("/change/") {
        id.to_string()
    } else {
        format!("/change/{id}")
    };
    Ok(json!({
        "__xml_root": "GetChangeResponse",
        "ChangeInfo": {
            "Id": normalized,
            "Status": "INSYNC",
            "SubmittedAt": now_iso(),
        }
    }))
}

pub fn get_health_check(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("HealthCheckId")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "HealthCheckId is required"))?;

    let hc = state
        .health_checks
        .get(id)
        .ok_or_else(|| AwsError::not_found("NoSuchHealthCheck", format!("No health check found with ID: {id}")))?;

    Ok(json!({
        "__xml_root": "GetHealthCheckResponse",
        "HealthCheck": {
            "Id": hc.id,
            "HealthCheckVersion": hc.health_check_version,
            "HealthCheckConfig": hc.config,
            "CallerReference": Uuid::new_v4().to_string(),
        }
    }))
}

pub fn get_query_logging_config(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;

    let cfg = state.query_logging_configs.get(id).ok_or_else(|| {
        AwsError::not_found("NoSuchQueryLoggingConfig", format!("No query logging config: {id}"))
    })?;

    Ok(json!({
        "__xml_root": "GetQueryLoggingConfigResponse",
        "QueryLoggingConfig": {
            "Id": cfg.id,
            "HostedZoneId": cfg.hosted_zone_id,
            "CloudWatchLogsLogGroupArn": cfg.cloud_watch_logs_log_group_arn,
        }
    }))
}

pub fn list_tags_for_resources(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let resource_type = input
        .get("ResourceType")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceType is required"))?;

    let ids: Vec<String> = input
        .get("ResourceIds")
        .and_then(|r| r.get("ResourceId"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut sets: Vec<Value> = Vec::new();
    for rid in ids {
        let tags: Vec<Value> = match resource_type {
            "hostedzone" => {
                let zid = resolve_zone_id(&rid);
                state
                    .hosted_zones
                    .get(&zid)
                    .map(|z| {
                        z.tags
                            .iter()
                            .map(|(k, v)| json!({ "Key": k, "Value": v }))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            "healthcheck" => vec![],
            _ => vec![],
        };
        sets.push(json!({
            "ResourceType": resource_type,
            "ResourceId": rid,
            "Tags": tags,
        }));
    }

    Ok(json!({
        "__xml_root": "ListTagsForResourcesResponse",
        "ResourceTagSets": {
            "ResourceTagSet": sets,
        }
    }))
}

pub fn get_geo_location(
    _state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let continent = input.get("ContinentCode").and_then(Value::as_str);
    let country = input.get("CountryCode").and_then(Value::as_str);
    let subdivision = input.get("SubdivisionCode").and_then(Value::as_str);

    let mut details = json!({});
    if let Some(c) = continent {
        details["ContinentCode"] = json!(c);
        details["ContinentName"] = json!("Continent");
    }
    if let Some(c) = country {
        details["CountryCode"] = json!(c);
        details["CountryName"] = json!("Country");
    }
    if let Some(s) = subdivision {
        details["SubdivisionCode"] = json!(s);
        details["SubdivisionName"] = json!("Subdivision");
    }

    Ok(json!({
        "__xml_root": "GetGeoLocationResponse",
        "GeoLocationDetails": details,
    }))
}

pub fn list_geo_locations(
    _state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let entries = vec![
        json!({ "ContinentCode": "AF", "ContinentName": "Africa" }),
        json!({ "ContinentCode": "AN", "ContinentName": "Antarctica" }),
        json!({ "ContinentCode": "AS", "ContinentName": "Asia" }),
        json!({ "ContinentCode": "EU", "ContinentName": "Europe" }),
        json!({ "ContinentCode": "NA", "ContinentName": "North America" }),
        json!({ "ContinentCode": "OC", "ContinentName": "Oceania" }),
        json!({ "ContinentCode": "SA", "ContinentName": "South America" }),
    ];
    Ok(json!({
        "__xml_root": "ListGeoLocationsResponse",
        "GeoLocationDetailsList": {
            "GeoLocationDetails": entries,
        },
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

pub fn list_reusable_delegation_sets(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sets: Vec<Value> = state
        .delegation_sets
        .iter()
        .map(|e| {
            let s = e.value();
            json!({
                "Id": s.id,
                "CallerReference": s.caller_reference,
                "NameServers": s.name_servers,
            })
        })
        .collect();
    Ok(json!({
        "__xml_root": "ListReusableDelegationSetsResponse",
        "DelegationSets": sets,
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

pub fn create_reusable_delegation_set(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let caller_reference = input
        .get("CallerReference")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "CallerReference is required"))?;
    let id = format!("/delegationset/{}", Uuid::new_v4());
    let ds = DelegationSet {
        id: id.clone(),
        caller_reference: caller_reference.to_string(),
        name_servers: vec![
            "ns-1.awsim.invalid".to_string(),
            "ns-2.awsim.invalid".to_string(),
            "ns-3.awsim.invalid".to_string(),
            "ns-4.awsim.invalid".to_string(),
        ],
    };
    state.delegation_sets.insert(id.clone(), ds.clone());
    Ok(json!({
        "__xml_root": "CreateReusableDelegationSetResponse",
        "DelegationSet": {
            "Id": ds.id,
            "CallerReference": ds.caller_reference,
            "NameServers": ds.name_servers,
        }
    }))
}

pub fn create_traffic_policy(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("Name")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Name is required"))?;
    let document = input
        .get("Document")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let comment = input.get("Comment").and_then(Value::as_str).map(String::from);
    let id = Uuid::new_v4().to_string();
    let tp = TrafficPolicy {
        id: id.clone(),
        name: name.to_string(),
        version: 1,
        document: document.to_string(),
        comment,
        r#type: "A".to_string(),
    };
    state.traffic_policies.insert(id.clone(), tp.clone());
    Ok(json!({
        "__xml_root": "CreateTrafficPolicyResponse",
        "TrafficPolicy": {
            "Id": tp.id,
            "Name": tp.name,
            "Version": tp.version,
            "Document": tp.document,
            "Comment": tp.comment,
            "Type": tp.r#type,
        }
    }))
}

pub fn get_traffic_policy(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let tp = state
        .traffic_policies
        .get(id)
        .ok_or_else(|| AwsError::not_found("NoSuchTrafficPolicy", format!("No traffic policy: {id}")))?;
    Ok(json!({
        "__xml_root": "GetTrafficPolicyResponse",
        "TrafficPolicy": {
            "Id": tp.id,
            "Name": tp.name,
            "Version": tp.version,
            "Document": tp.document,
            "Comment": tp.comment,
            "Type": tp.r#type,
        }
    }))
}

pub fn list_traffic_policies(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .traffic_policies
        .iter()
        .map(|e| {
            let t = e.value();
            json!({
                "Id": t.id,
                "Name": t.name,
                "Type": t.r#type,
                "LatestVersion": t.version,
                "TrafficPolicyCount": 1,
            })
        })
        .collect();
    Ok(json!({
        "__xml_root": "ListTrafficPoliciesResponse",
        "TrafficPolicySummaries": summaries,
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

pub fn delete_traffic_policy(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    if state.traffic_policies.remove(id).is_none() {
        return Err(AwsError::not_found(
            "NoSuchTrafficPolicy",
            format!("No traffic policy: {id}"),
        ));
    }
    Ok(json!({}))
}

pub fn associate_vpc_with_hosted_zone(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let zone_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let zone_id = resolve_zone_id(zone_raw);
    if !state.hosted_zones.contains_key(&zone_id) {
        return Err(AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone: {zone_id}"),
        ));
    }
    let vpc = input.get("VPC").cloned().unwrap_or_else(|| json!({}));
    let vpc_id = vpc.get("VPCId").and_then(Value::as_str).unwrap_or("vpc-unknown").to_string();
    let vpc_region = vpc.get("VPCRegion").and_then(Value::as_str).unwrap_or("us-east-1").to_string();
    state
        .vpc_associations
        .entry(zone_id.clone())
        .or_default()
        .push(VpcAssociation {
            vpc_id,
            vpc_region,
            hosted_zone_id: zone_id.clone(),
        });
    Ok(json!({
        "__xml_root": "AssociateVPCWithHostedZoneResponse",
        "ChangeInfo": {
            "Id": format!("/change/{}", Uuid::new_v4()),
            "Status": "INSYNC",
            "SubmittedAt": now_iso(),
        }
    }))
}

pub fn disassociate_vpc_from_hosted_zone(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let zone_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let zone_id = resolve_zone_id(zone_raw);
    let vpc = input.get("VPC").cloned().unwrap_or_else(|| json!({}));
    let vpc_id = vpc.get("VPCId").and_then(Value::as_str).unwrap_or("");
    if let Some(mut entry) = state.vpc_associations.get_mut(&zone_id) {
        entry.retain(|v| v.vpc_id != vpc_id);
    }
    Ok(json!({
        "__xml_root": "DisassociateVPCFromHostedZoneResponse",
        "ChangeInfo": {
            "Id": format!("/change/{}", Uuid::new_v4()),
            "Status": "INSYNC",
            "SubmittedAt": now_iso(),
        }
    }))
}
