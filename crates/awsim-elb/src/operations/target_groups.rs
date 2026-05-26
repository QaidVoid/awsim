use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::tg_arn,
    state::{AttributeKeyValue, ElbState, Target, TargetGroup},
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

/// Validate `Matcher.GrpcCode`. AWS accepts a single code, a hyphenated
/// range, or a comma-separated list — all values must fall in 0..=99.
fn validate_grpc_matcher(spec: &str) -> Result<(), AwsError> {
    if spec.is_empty() {
        return Err(AwsError::bad_request(
            "ValidationError",
            "Matcher.GrpcCode must not be empty.",
        ));
    }
    let codes = spec.split(',').flat_map(|piece| {
        let piece = piece.trim();
        if let Some((lo, hi)) = piece.split_once('-') {
            vec![lo.trim().to_string(), hi.trim().to_string()]
        } else {
            vec![piece.to_string()]
        }
    });
    for raw in codes {
        let code: u16 = raw.parse().map_err(|_| {
            AwsError::bad_request(
                "ValidationError",
                format!("Matcher.GrpcCode `{spec}` entry `{raw}` is not a valid integer."),
            )
        })?;
        if code > 99 {
            return Err(AwsError::bad_request(
                "ValidationError",
                format!("Matcher.GrpcCode `{spec}` entry `{raw}` must be in 0..=99."),
            ));
        }
    }
    Ok(())
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
    let target_type = opt_str(input, "TargetType")
        .unwrap_or("instance")
        .to_string();

    // HealthCheckProtocol must be a documented value. GRPC target
    // groups are matched against `Matcher.GrpcCode` rather than
    // HttpCode, and the range is documented as 0..=99.
    let hc_protocol = opt_str(input, "HealthCheckProtocol").unwrap_or(&protocol);
    if !matches!(hc_protocol, "HTTP" | "HTTPS" | "TCP" | "GRPC") {
        return Err(AwsError::bad_request(
            "ValidationError",
            format!(
                "HealthCheckProtocol `{hc_protocol}` is not valid. \
                 Allowed: HTTP, HTTPS, TCP, GRPC."
            ),
        ));
    }
    if let Some(matcher) = input.get("Matcher")
        && let Some(grpc) = matcher.get("GrpcCode").and_then(Value::as_str)
    {
        validate_grpc_matcher(grpc)?;
    }

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
        "TargetGroups": {
            "member": [result]
        }
    }))
}

pub fn delete_target_group(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "TargetGroupArn")?;

    if state.target_groups.remove(arn).is_none() {
        return Err(resource_not_found("target group", arn));
    }

    Ok(json!({}))
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
        "TargetGroups": {
            "member": tgs
        },
        "NextMarker": null
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

    Ok(json!({}))
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

    Ok(json!({}))
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
        "TargetHealthDescriptions": {
            "member": descriptions
        }
    }))
}

pub fn describe_target_group_attributes(
    state: &ElbState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "TargetGroupArn")?;

    if !state.target_groups.contains_key(arn) {
        return Err(resource_not_found("target group", arn));
    }

    let stored = state.tg_attributes.get(arn);
    let attrs: Vec<Value> = if let Some(ref kv_list) = stored {
        kv_list
            .iter()
            .map(|kv| json!({ "Key": kv.key, "Value": kv.value }))
            .collect()
    } else {
        default_tg_attributes()
    };

    Ok(json!({
        "Attributes": { "member": attrs }
    }))
}

fn default_tg_attributes() -> Vec<Value> {
    vec![
        json!({ "Key": "deregistration_delay.timeout_seconds", "Value": "300" }),
        json!({ "Key": "stickiness.enabled", "Value": "false" }),
        json!({ "Key": "stickiness.type", "Value": "lb_cookie" }),
        json!({ "Key": "stickiness.lb_cookie.duration_seconds", "Value": "86400" }),
        json!({ "Key": "load_balancing.algorithm.type", "Value": "round_robin" }),
        json!({ "Key": "slow_start.duration_seconds", "Value": "0" }),
    ]
}

fn parse_attribute_list(input: &Value) -> Vec<AttributeKeyValue> {
    let mut result = Vec::new();
    if let Some(attrs) = input.get("Attributes") {
        let items: Vec<&Value> = match attrs {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => {
                if let Some(Value::Object(m)) = map.get("member") {
                    m.values().collect()
                } else {
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    pairs.into_iter().map(|(_, v)| v).collect()
                }
            }
            _ => vec![],
        };
        for item in items {
            let key = item
                .get("Key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let value = item
                .get("Value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            result.push(AttributeKeyValue { key, value });
        }
    }
    result
}

pub fn modify_target_group_attributes(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "TargetGroupArn")?;

    if !state.target_groups.contains_key(arn) {
        return Err(resource_not_found("target group", arn));
    }

    let kv_list = parse_attribute_list(input);
    let attrs: Vec<Value> = kv_list
        .iter()
        .map(|kv| json!({ "Key": kv.key, "Value": kv.value }))
        .collect();
    state.tg_attributes.insert(arn.to_string(), kv_list);

    Ok(json!({
        "Attributes": { "member": attrs }
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
                        targets.push(Target {
                            id: id.to_string(),
                            port,
                        });
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
                        targets.push(Target {
                            id: id.to_string(),
                            port,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    targets
}

#[cfg(test)]
mod grpc_matcher_tests {
    use super::*;

    #[test]
    fn accepts_single_code() {
        validate_grpc_matcher("0").unwrap();
        validate_grpc_matcher("99").unwrap();
    }

    #[test]
    fn accepts_range() {
        validate_grpc_matcher("0-99").unwrap();
        validate_grpc_matcher("3-15").unwrap();
    }

    #[test]
    fn accepts_comma_list() {
        validate_grpc_matcher("0,3,16").unwrap();
        validate_grpc_matcher("0,3-15,99").unwrap();
    }

    #[test]
    fn rejects_code_above_99() {
        let err = validate_grpc_matcher("100").unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn rejects_empty() {
        let err = validate_grpc_matcher("").unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn rejects_non_numeric() {
        let err = validate_grpc_matcher("abc").unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    fn ctx() -> RequestContext {
        RequestContext::new("elasticloadbalancing", "us-east-1")
    }

    #[test]
    fn create_target_group_rejects_unknown_hc_protocol() {
        let state = ElbState::default();
        let err = create_target_group(
            &state,
            &json!({
                "Name": "tg",
                "Protocol": "HTTP",
                "Port": 80,
                "HealthCheckProtocol": "GOPHER",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationError");
    }

    #[test]
    fn create_target_group_accepts_grpc_matcher() {
        let state = ElbState::default();
        create_target_group(
            &state,
            &json!({
                "Name": "tg",
                "Protocol": "HTTP",
                "Port": 80,
                "HealthCheckProtocol": "GRPC",
                "Matcher": { "GrpcCode": "0-99" },
            }),
            &ctx(),
        )
        .unwrap();
    }
}
