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
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Enforce AWS's routing-policy rules: at most one of the
/// policy-specific fields may be present, and any policy-specific
/// field requires a `SetIdentifier` to tie it back to the policy
/// instance. Real AWS rejects mismatches with InvalidChangeBatch.
fn validate_routing_policy(rs: &Value) -> Result<(), AwsError> {
    let weight = rs.get("Weight").and_then(Value::as_i64);
    let failover = rs.get("Failover").and_then(Value::as_str);
    let region = rs.get("Region").and_then(Value::as_str);
    let geo_location = rs.get("GeoLocation");
    let geo_proximity = rs.get("GeoProximityLocation");
    let multi_value = rs.get("MultiValueAnswer").and_then(Value::as_bool);

    let set_count = [
        weight.is_some(),
        failover.is_some(),
        region.is_some(),
        geo_location.is_some(),
        geo_proximity.is_some(),
        multi_value.is_some(),
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if set_count > 1 {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            "A ResourceRecordSet may specify only one routing policy: \
             Weight, Failover, Region, GeoLocation, GeoProximityLocation, \
             or MultiValueAnswer.",
        ));
    }
    if set_count == 1
        && rs
            .get("SetIdentifier")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .is_none()
    {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            "Routing-policy records must also include a non-empty SetIdentifier.",
        ));
    }
    if let Some(w) = weight
        && !(0..=255).contains(&w)
    {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            format!("Weight {w} must be between 0 and 255."),
        ));
    }
    if let Some(f) = failover
        && !matches!(f, "PRIMARY" | "SECONDARY")
    {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            format!("Failover `{f}` must be PRIMARY or SECONDARY."),
        ));
    }
    Ok(())
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

    validate_routing_policy(rs)?;

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

    let mut zone = state.hosted_zones.get_mut(&id).ok_or_else(|| {
        AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {id}"),
        )
    })?;

    // Changes are in ChangeBatch.Changes.Change (array)
    let changes = input
        .get("ChangeBatch")
        .and_then(|cb| cb.get("Changes"))
        .and_then(|ch| ch.get("Change"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // AWS caps a single ChangeResourceRecordSets call at 1000 changes
    // and 32_000 ResourceRecord values across the batch. Real Route53
    // rejects with InvalidChangeBatch when either limit is exceeded.
    const MAX_CHANGES_PER_BATCH: usize = 1000;
    const MAX_VALUES_PER_BATCH: usize = 32_000;
    if changes.len() > MAX_CHANGES_PER_BATCH {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            format!(
                "ChangeBatch contains {} changes; maximum is {MAX_CHANGES_PER_BATCH}.",
                changes.len()
            ),
        ));
    }
    let total_values: usize = changes
        .iter()
        .filter_map(|c| {
            c.get("ResourceRecordSet")
                .and_then(|rs| rs.get("ResourceRecords"))
                .and_then(|rr| rr.get("ResourceRecord"))
                .and_then(Value::as_array)
                .map(|v| v.len())
        })
        .sum();
    if total_values > MAX_VALUES_PER_BATCH {
        return Err(AwsError::bad_request(
            "InvalidChangeBatch",
            format!(
                "ChangeBatch contains {total_values} ResourceRecord values; maximum is {MAX_VALUES_PER_BATCH}."
            ),
        ));
    }

    for change in &changes {
        let action = change
            .get("Action")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::bad_request("InvalidInput", "Action is required"))?;
        let rs_value = change.get("ResourceRecordSet").ok_or_else(|| {
            AwsError::bad_request("InvalidInput", "ResourceRecordSet is required")
        })?;
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

    // Stamp the submission so GetChange can walk PENDING -> INSYNC
    // after a short propagation window. The change id is bare here
    // (no `/change/` prefix); GetChange strips that prefix before
    // looking up.
    let change_id = Uuid::new_v4().to_string();
    state.change_submissions.insert(
        change_id.clone(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );

    Ok(json!({
        "__xml_root": "ChangeResourceRecordSetsResponse",
        "ChangeInfo": {
            "Id": format!("/change/{change_id}"),
            "Status": "PENDING",
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

    let zone = state.hosted_zones.get(&id).ok_or_else(|| {
        AwsError::not_found(
            "NoSuchHostedZone",
            format!("No hosted zone found with ID: {id}"),
        )
    })?;

    // AWS bounds MaxItems at 1..=300. Both string and integer forms
    // appear in real requests.
    let max_items: usize = match input.get("MaxItems") {
        None => 100,
        Some(v) => {
            let n = v
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
                .ok_or_else(|| {
                    AwsError::bad_request("InvalidInput", "MaxItems must be a positive integer")
                })?;
            if !(1..=300).contains(&n) {
                return Err(AwsError::bad_request(
                    "InvalidInput",
                    format!("MaxItems `{n}` must be in 1..=300."),
                ));
            }
            n as usize
        }
    };
    let start_name = input.get("StartRecordName").and_then(Value::as_str);
    let start_type = input.get("StartRecordType").and_then(Value::as_str);
    let mut ordered: Vec<&_> = zone.record_sets.iter().collect();
    // AWS orders by (Name, Type). Name compares lexicographically with
    // the trailing dot kept; Type is alphabetical (A < AAAA < CNAME).
    ordered.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.r#type.cmp(&b.r#type)));
    let start_idx = match start_name {
        None => 0,
        Some(name) => ordered
            .iter()
            .position(|rs| {
                rs.name.as_str() > name
                    || (rs.name == name && start_type.is_none_or(|t| rs.r#type.as_str() >= t))
            })
            .unwrap_or(ordered.len()),
    };
    let total = ordered.len();
    let end_idx = (start_idx + max_items).min(total);
    let is_truncated = end_idx < total;
    let record_sets: Vec<Value> = ordered[start_idx..end_idx]
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

    let mut body = json!({
        "__xml_root": "ListResourceRecordSetsResponse",
        "ResourceRecordSets": record_sets,
        "IsTruncated": is_truncated,
        "MaxItems": max_items.to_string(),
    });
    if is_truncated {
        body["NextRecordName"] = json!(ordered[end_idx].name);
        body["NextRecordType"] = json!(ordered[end_idx].r#type);
    }
    Ok(body)
}

#[cfg(test)]
mod list_record_sets_tests {
    use super::*;
    use crate::state::{HostedZone, ResourceRecordSet};
    use std::collections::HashMap;

    fn ctx() -> RequestContext {
        RequestContext::new("route53", "us-east-1")
    }

    fn make_state_with_records(names: &[&str]) -> Arc<Route53State> {
        let state = Arc::new(Route53State::default());
        let zone = HostedZone {
            id: "/hostedzone/Z1".to_string(),
            name: "example.com.".to_string(),
            caller_reference: "ref".to_string(),
            record_sets: names
                .iter()
                .map(|n| ResourceRecordSet {
                    name: (*n).to_string(),
                    r#type: "A".to_string(),
                    ttl: Some(60),
                    resource_records: vec!["1.2.3.4".to_string()],
                    alias_target: None,
                })
                .collect(),
            tags: HashMap::new(),
            created_at: "2024".to_string(),
            private_zone: false,
            vpcs: vec![],
            comment: None,
        };
        state.hosted_zones.insert(zone.id.clone(), zone);
        state
    }

    #[test]
    fn list_paginates_with_max_items_and_start_record_name() {
        let state =
            make_state_with_records(&["a.example.", "b.example.", "c.example.", "d.example."]);
        let first =
            list_resource_record_sets(&state, &json!({ "Id": "Z1", "MaxItems": "2" }), &ctx())
                .unwrap();
        assert_eq!(first["IsTruncated"], true);
        assert_eq!(first["NextRecordName"], "c.example.");
        let second = list_resource_record_sets(
            &state,
            &json!({
                "Id": "Z1",
                "MaxItems": "2",
                "StartRecordName": "c.example.",
            }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<&str> = second["ResourceRecordSets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["c.example.", "d.example."]);
        assert_eq!(second["IsTruncated"], false);
    }

    #[test]
    fn list_rejects_max_items_out_of_range() {
        let state = make_state_with_records(&["a.example."]);
        let err =
            list_resource_record_sets(&state, &json!({ "Id": "Z1", "MaxItems": "500" }), &ctx())
                .unwrap_err();
        assert_eq!(err.code, "InvalidInput");
    }
}

#[cfg(test)]
mod routing_policy_tests {
    use super::*;

    fn rs(payload: serde_json::Value) -> serde_json::Value {
        let mut base = json!({ "Name": "x.example.", "Type": "A" });
        let obj = base.as_object_mut().unwrap();
        for (k, v) in payload.as_object().unwrap() {
            obj.insert(k.clone(), v.clone());
        }
        base
    }

    #[test]
    fn simple_record_passes() {
        validate_routing_policy(&rs(json!({}))).unwrap();
    }

    #[test]
    fn weighted_requires_set_identifier() {
        let err = validate_routing_policy(&rs(json!({ "Weight": 50 }))).unwrap_err();
        assert_eq!(err.code, "InvalidChangeBatch");
        assert!(err.message.contains("SetIdentifier"));
    }

    #[test]
    fn weighted_with_set_identifier_passes() {
        validate_routing_policy(&rs(json!({ "Weight": 50, "SetIdentifier": "v1" }))).unwrap();
    }

    #[test]
    fn rejects_weight_out_of_range() {
        let err = validate_routing_policy(&rs(json!({ "Weight": 999, "SetIdentifier": "v1" })))
            .unwrap_err();
        assert_eq!(err.code, "InvalidChangeBatch");
    }

    #[test]
    fn multiple_policies_rejected() {
        let err = validate_routing_policy(&rs(json!({
            "Weight": 50,
            "Failover": "PRIMARY",
            "SetIdentifier": "v1",
        })))
        .unwrap_err();
        assert_eq!(err.code, "InvalidChangeBatch");
    }

    #[test]
    fn failover_must_be_primary_or_secondary() {
        let err = validate_routing_policy(&rs(json!({
            "Failover": "TERTIARY",
            "SetIdentifier": "v1",
        })))
        .unwrap_err();
        assert_eq!(err.code, "InvalidChangeBatch");
    }
}
