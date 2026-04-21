use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::tg_arn,
    state::{ElbState, Target, TargetGroup},
};

use super::{extract_string_list, opt_str, require_str};

pub fn tg_to_value(tg: &TargetGroup) -> Value {
    json!({
        "TargetGroupArn": tg.arn,
        "TargetGroupName": tg.name,
        "Protocol": tg.protocol,
        "Port": tg.port,
        "VpcId": tg.vpc_id,
        "HealthCheckProtocol": tg.protocol,
        "HealthCheckPath": "/",
        "HealthCheckIntervalSeconds": 30,
        "HealthCheckTimeoutSeconds": 5,
        "HealthyThresholdCount": 5,
        "UnhealthyThresholdCount": 2,
        "Matcher": { "HttpCode": "200" },
        "LoadBalancerArns": [],
        "TargetType": tg.target_type,
        "IpAddressType": "ipv4",
    })
}

pub fn create_target_group(
    state: &ElbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?.to_string();

    if state.target_groups.iter().any(|e| e.value().name == name) {
        return Err(AwsError::conflict(
            "DuplicateTargetGroupName",
            format!("A target group named '{name}' already exists"),
        ));
    }

    let protocol = opt_str(input, "Protocol").unwrap_or("HTTP").to_string();
    let port: u16 = input
        .get("Port")
        .and_then(|v| match v {
            Value::Number(n) => n.as_u64().map(|n| n as u16),
            Value::String(s) => s.parse().ok(),
            _ => None,
        })
        .unwrap_or(80);
    let vpc_id = opt_str(input, "VpcId").unwrap_or("").to_string();
    let target_type = opt_str(input, "TargetType").unwrap_or("instance").to_string();

    let arn = tg_arn(&ctx.region, &ctx.account_id, &name);

    let tg = TargetGroup {
        arn: arn.clone(),
        name,
        protocol,
        port,
        vpc_id,
        target_type,
        targets: Vec::new(),
        tags: HashMap::new(),
    };

    let result = tg_to_value(&tg);
    state.target_groups.insert(arn, tg);

    Ok(json!({
        "CreateTargetGroupResult": {
            "TargetGroups": {
                "member": [result]
            }
        }
    }))
}

pub fn delete_target_group(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "TargetGroupArn")?;

    if state.target_groups.remove(arn).is_none() {
        return Err(resource_not_found("target group", arn));
    }

    Ok(json!({ "DeleteTargetGroupResult": {} }))
}

pub fn describe_target_groups(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn_filter = extract_string_list(input, "TargetGroupArns");
    let name_filter = extract_string_list(input, "Names");

    let tgs: Vec<Value> = state
        .target_groups
        .iter()
        .filter(|e| {
            let tg = e.value();
            let arn_ok = arn_filter.is_empty() || arn_filter.contains(&tg.arn);
            let name_ok = name_filter.is_empty() || name_filter.contains(&tg.name);
            arn_ok && name_ok
        })
        .map(|e| tg_to_value(e.value()))
        .collect();

    Ok(json!({
        "DescribeTargetGroupsResult": {
            "TargetGroups": {
                "member": tgs
            },
            "NextMarker": null
        }
    }))
}

pub fn register_targets(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let tg_arn = require_str(input, "TargetGroupArn")?;

    let mut entry = state
        .target_groups
        .get_mut(tg_arn)
        .ok_or_else(|| resource_not_found("target group", tg_arn))?;

    // Parse Targets.member.N or Targets as array
    let new_targets = parse_targets(input);

    for t in new_targets {
        // Avoid duplicate registrations
        if !entry.targets.iter().any(|existing| existing.id == t.id) {
            entry.targets.push(t);
        }
    }

    Ok(json!({ "RegisterTargetsResult": {} }))
}

pub fn deregister_targets(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let tg_arn = require_str(input, "TargetGroupArn")?;

    let mut entry = state
        .target_groups
        .get_mut(tg_arn)
        .ok_or_else(|| resource_not_found("target group", tg_arn))?;

    let remove_targets = parse_targets(input);
    let remove_ids: Vec<String> = remove_targets.into_iter().map(|t| t.id).collect();
    entry.targets.retain(|t| !remove_ids.contains(&t.id));

    Ok(json!({ "DeregisterTargetsResult": {} }))
}

pub fn describe_target_health(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let tg_arn = require_str(input, "TargetGroupArn")?;

    let entry = state
        .target_groups
        .get(tg_arn)
        .ok_or_else(|| resource_not_found("target group", tg_arn))?;

    let descriptions: Vec<Value> = entry
        .targets
        .iter()
        .map(|t| {
            json!({
                "Target": {
                    "Id": t.id,
                    "Port": t.port.unwrap_or(entry.port),
                },
                "HealthCheckPort": t.port.unwrap_or(entry.port).to_string(),
                "TargetHealth": {
                    "State": "healthy",
                    "Reason": null,
                    "Description": null,
                }
            })
        })
        .collect();

    Ok(json!({
        "DescribeTargetHealthResult": {
            "TargetHealthDescriptions": {
                "member": descriptions
            }
        }
    }))
}

fn parse_targets(input: &Value) -> Vec<Target> {
    let mut targets = Vec::new();

    if let Some(t) = input.get("Targets") {
        match t {
            Value::Array(arr) => {
                for item in arr {
                    if let Some(id) = item.get("Id").and_then(|v| v.as_str()) {
                        let port = item.get("Port").and_then(|v| match v {
                            Value::Number(n) => n.as_u64().map(|n| n as u16),
                            Value::String(s) => s.parse().ok(),
                            _ => None,
                        });
                        targets.push(Target { id: id.to_string(), port });
                    }
                }
            }
            Value::Object(map) => {
                // member.1, member.2, etc.
                let members = if let Some(Value::Object(m)) = map.get("member") {
                    m
                } else {
                    map
                };
                let mut pairs: Vec<_> = members.iter().collect();
                pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                for (_, v) in pairs {
                    if let Some(id) = v.get("Id").and_then(|v| v.as_str()) {
                        let port = v.get("Port").and_then(|p| match p {
                            Value::Number(n) => n.as_u64().map(|n| n as u16),
                            Value::String(s) => s.parse().ok(),
                            _ => None,
                        });
                        targets.push(Target { id: id.to_string(), port });
                    }
                }
            }
            _ => {}
        }
    }

    targets
}
