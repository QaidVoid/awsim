use std::sync::Arc;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{HostedZone, ResourceRecordSet, Route53State};

/// Ensure a zone name ends with `.`
fn normalize_name(name: &str) -> String {
    if name.ends_with('.') {
        name.to_string()
    } else {
        format!("{name}.")
    }
}

/// Build default SOA and NS records for a new hosted zone.
fn default_records(name: &str) -> Vec<ResourceRecordSet> {
    vec![
        ResourceRecordSet {
            name: name.to_string(),
            r#type: "SOA".to_string(),
            ttl: Some(900),
            resource_records: vec![format!(
                "ns-1.awsim.invalid. awsdns-hostmaster.amazon.com. 1 7200 900 1209600 86400"
            )],
            alias_target: None,
        },
        ResourceRecordSet {
            name: name.to_string(),
            r#type: "NS".to_string(),
            ttl: Some(172800),
            resource_records: vec![
                "ns-1.awsim.invalid.".to_string(),
                "ns-2.awsim.invalid.".to_string(),
                "ns-3.awsim.invalid.".to_string(),
                "ns-4.awsim.invalid.".to_string(),
            ],
            alias_target: None,
        },
    ]
}

/// POST /2013-04-01/hostedzone
pub fn create_hosted_zone(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input
        .get("Name")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Name is required"))?;
    let caller_reference = input
        .get("CallerReference")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "CallerReference is required"))?;

    let name = normalize_name(name);
    let id = format!("/hostedzone/{}", Uuid::new_v4());
    let now = chrono_now();

    let zone = HostedZone {
        id: id.clone(),
        name: name.clone(),
        caller_reference: caller_reference.to_string(),
        record_sets: default_records(&name),
        tags: std::collections::HashMap::new(),
        created_at: now,
    };

    state.hosted_zones.insert(id.clone(), zone);

    Ok(json!({
        "__xml_root": "CreateHostedZoneResponse",
        "HostedZone": {
            "Id": id,
            "Name": name,
            "CallerReference": caller_reference,
            "Config": { "PrivateZone": false },
            "ResourceRecordSetCount": 2,
        },
        "ChangeInfo": {
            "Id": format!("/change/{}", Uuid::new_v4()),
            "Status": "INSYNC",
            "SubmittedAt": chrono_now(),
        },
        "DelegationSet": {
            "NameServers": [
                "ns-1.awsim.invalid",
                "ns-2.awsim.invalid",
                "ns-3.awsim.invalid",
                "ns-4.awsim.invalid",
            ]
        }
    }))
}

/// GET /2013-04-01/hostedzone/{Id}
pub fn get_hosted_zone(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;

    // The path param may be just the UUID portion; normalize to full path.
    let id = if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    };

    let zone = state.hosted_zones.get(&id).ok_or_else(|| {
        AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {id}"),
        )
    })?;

    Ok(json!({
        "__xml_root": "GetHostedZoneResponse",
        "HostedZone": {
            "Id": zone.id,
            "Name": zone.name,
            "CallerReference": zone.caller_reference,
            "Config": { "PrivateZone": false },
            "ResourceRecordSetCount": zone.record_sets.len(),
        },
        "DelegationSet": {
            "NameServers": [
                "ns-1.awsim.invalid",
                "ns-2.awsim.invalid",
                "ns-3.awsim.invalid",
                "ns-4.awsim.invalid",
            ]
        }
    }))
}

/// GET /2013-04-01/hostedzone
pub fn list_hosted_zones(
    state: &Arc<Route53State>,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let zones: Vec<Value> = state
        .hosted_zones
        .iter()
        .map(|entry| {
            let z = entry.value();
            json!({
                "Id": z.id,
                "Name": z.name,
                "CallerReference": z.caller_reference,
                "Config": { "PrivateZone": false },
                "ResourceRecordSetCount": z.record_sets.len(),
            })
        })
        .collect();

    Ok(json!({
        "__xml_root": "ListHostedZonesResponse",
        "HostedZones": zones,
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

/// DELETE /2013-04-01/hostedzone/{Id}
pub fn delete_hosted_zone(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id_raw = input
        .get("Id")
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidInput", "Id is required"))?;

    let id = if id_raw.starts_with("/hostedzone/") {
        id_raw.to_string()
    } else {
        format!("/hostedzone/{id_raw}")
    };

    if state.hosted_zones.remove(&id).is_none() {
        return Err(AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {id}"),
        ));
    }

    Ok(json!({
        "__xml_root": "DeleteHostedZoneResponse",
        "ChangeInfo": {
            "Id": format!("/change/{}", Uuid::new_v4()),
            "Status": "INSYNC",
            "SubmittedAt": chrono_now(),
        }
    }))
}

/// GET /2013-04-01/hostedzonesbyname
pub fn list_hosted_zones_by_name(
    state: &Arc<Route53State>,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let dns_name = input
        .get("DNSName")
        .and_then(Value::as_str)
        .map(normalize_name);

    let mut zones: Vec<Value> = state
        .hosted_zones
        .iter()
        .filter(|entry| {
            dns_name
                .as_deref()
                .map(|n| entry.value().name == n)
                .unwrap_or(true)
        })
        .map(|entry| {
            let z = entry.value();
            json!({
                "Id": z.id,
                "Name": z.name,
                "CallerReference": z.caller_reference,
                "Config": { "PrivateZone": false },
                "ResourceRecordSetCount": z.record_sets.len(),
            })
        })
        .collect();

    zones.sort_by(|a, b| a["Name"].as_str().cmp(&b["Name"].as_str()));

    Ok(json!({
        "HostedZones": zones,
        "IsTruncated": false,
        "MaxItems": "100",
    }))
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // ISO8601 UTC with seconds precision
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // Simplified Gregorian calendar conversion
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
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
