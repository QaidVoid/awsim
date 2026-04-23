use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::{lb_arn, lb_dns_name, now_iso8601},
    state::{AttributeKeyValue, ElbState, LoadBalancer},
};

use super::{extract_string_list, opt_str, require_str};

pub fn lb_to_value(lb: &LoadBalancer) -> Value {
    json!({
        "LoadBalancerArn": lb.arn,
        "DNSName": lb.dns_name,
        "CanonicalHostedZoneId": "Z35SXDOTRQ7X7K",
        "CreatedTime": lb.created_at,
        "LoadBalancerName": lb.name,
        "Scheme": lb.scheme,
        "VpcId": lb.vpc_id,
        "State": { "Code": lb.state },
        "Type": lb.lb_type,
        "AvailabilityZones": [],
        "SecurityGroups": lb.security_groups,
        "IpAddressType": "ipv4",
    })
}

pub fn create_load_balancer(
    state: &ElbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?.to_string();

    // Check for duplicate name
    if state
        .load_balancers
        .iter()
        .any(|e| e.value().name == name)
    {
        return Err(AwsError::conflict(
            "DuplicateLoadBalancerName",
            format!("A load balancer named '{name}' already exists"),
        ));
    }

    let lb_type = opt_str(input, "Type").unwrap_or("application").to_string();
    let scheme = opt_str(input, "Scheme")
        .unwrap_or("internet-facing")
        .to_string();
    let subnets = extract_string_list(input, "Subnets");
    let security_groups = extract_string_list(input, "SecurityGroups");
    let vpc_id = opt_str(input, "VpcId").unwrap_or("").to_string();

    let arn = lb_arn(&ctx.region, &ctx.account_id, &lb_type, &name);
    let dns_name = lb_dns_name(&name, &ctx.region);

    let lb = LoadBalancer {
        arn: arn.clone(),
        name,
        dns_name,
        lb_type,
        scheme,
        state: "active".to_string(),
        subnets,
        security_groups,
        tags: HashMap::new(),
        created_at: now_iso8601(),
        vpc_id,
    };

    let result = lb_to_value(&lb);
    state.load_balancers.insert(arn, lb);

    Ok(json!({
        "LoadBalancers": {
            "member": [result]
        }
    }))
}

pub fn delete_load_balancer(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    if state.load_balancers.remove(arn).is_none() {
        return Err(resource_not_found("load balancer", arn));
    }

    // Remove associated listeners and rules
    state
        .listeners
        .retain(|_, v| v.load_balancer_arn != arn);
    state.rules.retain(|_, v| {
        state.listeners.contains_key(&v.listener_arn)
    });

    Ok(json!({}))
}

pub fn describe_load_balancers(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn_filter = extract_string_list(input, "LoadBalancerArns");
    let name_filter = extract_string_list(input, "Names");

    let lbs: Vec<Value> = state
        .load_balancers
        .iter()
        .filter(|e| {
            let lb = e.value();
            let arn_ok = arn_filter.is_empty() || arn_filter.contains(&lb.arn);
            let name_ok = name_filter.is_empty() || name_filter.contains(&lb.name);
            arn_ok && name_ok
        })
        .map(|e| lb_to_value(e.value()))
        .collect();

    Ok(json!({
        "LoadBalancers": {
            "member": lbs
        },
        "NextMarker": null
    }))
}

pub fn describe_load_balancer_attributes(
    state: &ElbState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    if !state.load_balancers.contains_key(arn) {
        return Err(resource_not_found("load balancer", arn));
    }

    // Return stored attributes, falling back to defaults
    let stored = state.lb_attributes.get(arn);
    let attrs: Vec<Value> = if let Some(ref kv_list) = stored {
        kv_list
            .iter()
            .map(|kv| json!({ "Key": kv.key, "Value": kv.value }))
            .collect()
    } else {
        default_lb_attributes()
    };

    Ok(json!({
        "Attributes": { "member": attrs }
    }))
}

fn default_lb_attributes() -> Vec<Value> {
    vec![
        json!({ "Key": "access_logs.s3.enabled", "Value": "false" }),
        json!({ "Key": "access_logs.s3.bucket", "Value": "" }),
        json!({ "Key": "access_logs.s3.prefix", "Value": "" }),
        json!({ "Key": "deletion_protection.enabled", "Value": "false" }),
        json!({ "Key": "idle_timeout.timeout_seconds", "Value": "60" }),
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

pub fn modify_load_balancer_attributes(
    state: &ElbState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    if !state.load_balancers.contains_key(arn) {
        return Err(resource_not_found("load balancer", arn));
    }

    let kv_list = parse_attribute_list(input);
    let attrs: Vec<Value> = kv_list
        .iter()
        .map(|kv| json!({ "Key": kv.key, "Value": kv.value }))
        .collect();
    state.lb_attributes.insert(arn.to_string(), kv_list);

    Ok(json!({
        "Attributes": { "member": attrs }
    }))
}

pub fn set_security_groups(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    let mut lb = state
        .load_balancers
        .get_mut(arn)
        .ok_or_else(|| resource_not_found("load balancer", arn))?;

    lb.security_groups = extract_string_list(input, "SecurityGroups");
    let sgs = lb.security_groups.clone();

    Ok(json!({
        "SecurityGroupIds": { "member": sgs }
    }))
}

pub fn set_subnets(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    let mut lb = state
        .load_balancers
        .get_mut(arn)
        .ok_or_else(|| resource_not_found("load balancer", arn))?;

    lb.subnets = extract_string_list(input, "Subnets");
    let subnets: Vec<Value> = lb
        .subnets
        .iter()
        .map(|s| json!({ "SubnetId": s, "ZoneName": "us-east-1a" }))
        .collect();

    Ok(json!({
        "AvailabilityZones": { "member": subnets }
    }))
}
