use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{AliasTarget, ResourceRecordSet, Route53State};

fn resolve_zone_id(id_raw: &str) -> String {
    if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for &md in month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    (year, month + 1, remaining + 1, h, m, s)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Parse a Change element from the XML-decoded input JSON.
fn parse_record_set(rs: &Value) -> Result<ResourceRecordSet, AwsError> {
    let name = rs
        .get("Name")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceRecordSet.Name is required"))?
        .to_string();
    let record_type = rs
        .get("Type")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceRecordSet.Type is required"))?
        .to_string();
    let ttl = rs.get("TTL").and_then(Value::as_u64);

    // Resource records may be in ResourceRecords.ResourceRecord (array)
    let resource_records: Vec<String> = rs
        .get("ResourceRecords")
        .and_then(|rr| rr.get("ResourceRecord"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("Value").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let alias_target = rs.get("AliasTarget").map(|at| AliasTarget {
        dns_name: at
            .get("DNSName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        evaluate_target_health: at
            .get("EvaluateTargetHealth")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        hosted_zone_id: at
            .get("HostedZoneId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    });

    Ok(ResourceRecordSet {
        name,
        r#type: record_type,
        ttl,
        resource_records,
        alias_target,
    })
}

/// POST /2013-04-01/hostedzone/{Id}/rrset
pub fn change_resource_record_sets(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let id = resolve_zone_id(id_raw);

    let mut zone = state
        .hosted_zones
        .get_mut(&id)
        .ok_or_else(|| AwsError::not_found("NoSuchHostedZone", format!("No hosted zone found with ID: {id}")))?;

    // Changes are in ChangeBatch.Changes.Change (array)
    let changes = input
        .get("ChangeBatch")
        .and_then(|cb| cb.get("Changes"))
        .and_then(|ch| ch.get("Change"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for change in &changes {
        let action = change
            .get("Action")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::bad_request("InvalidInput", "Action is required"))?;
        let rs_value = change
            .get("ResourceRecordSet")
            .ok_or_else(|| AwsError::bad_request("InvalidInput", "ResourceRecordSet is required"))?;
        let rrs = parse_record_set(rs_value)?;

        match action {
            "CREATE" | "UPSERT" => {
                // Remove existing record with same name+type
                zone.record_sets
                    .retain(|r| !(r.name == rrs.name && r.r#type == rrs.r#type));
                zone.record_sets.push(rrs);
            }
            "DELETE" => {
                zone.record_sets
                    .retain(|r| !(r.name == rrs.name && r.r#type == rrs.r#type));
            }
            other => {
                return Err(AwsError::bad_request(
                    "InvalidInput",
                    format!("Unknown action: {other}"),
                ));
            }
        }
    }

    Ok(json!({
        "__xml_root": "ChangeResourceRecordSetsResponse",
        "ChangeInfo": {
            "Id": format!("/change/{}", Uuid::new_v4()),
            "Status": "INSYNC",
            "SubmittedAt": chrono_now(),
            "Comment": "",
        }
    }))
}

/// GET /2013-04-01/hostedzone/{Id}/rrset
pub fn list_resource_record_sets(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;
    let id = resolve_zone_id(id_raw);

    let zone = state
        .hosted_zones
        .get(&id)
        .ok_or_else(|| AwsError::not_found("NoSuchHostedZone", format!("No hosted zone found with ID: {id}")))?;

    let record_sets: Vec<Value> = zone
        .record_sets
        .iter()
        .map(|rs| {
            let mut obj = json!({
                "Name": rs.name,
                "Type": rs.r#type,
            });
            if let Some(ttl) = rs.ttl {
                obj["TTL"] = json!(ttl);
            }
            if !rs.resource_records.is_empty() {
                obj["ResourceRecords"] = json!({
                    "ResourceRecord": rs.resource_records.iter().map(|v| json!({"Value": v})).collect::<Vec<_>>()
                });
            }
            if let Some(at) = &rs.alias_target {
                obj["AliasTarget"] = json!({
                    "DNSName": at.dns_name,
                    "EvaluateTargetHealth": at.evaluate_target_health,
                    "HostedZoneId": at.hosted_zone_id,
                });
            }
            obj
        })
        .collect();

    Ok(json!({
        "__xml_root": "ListResourceRecordSetsResponse",
        "ResourceRecordSets": record_sets,
        "IsTruncated": false,
        "MaxItems": "300",
    }))
}
