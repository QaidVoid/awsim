use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::{lb_arn, lb_dns_name, now_iso8601},
    state::{ElbState, LoadBalancer},
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
        "CreateLoadBalancerResult": {
            "LoadBalancers": {
                "member": [result]
            }
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

    Ok(json!({ "DeleteLoadBalancerResult": {} }))
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
        "DescribeLoadBalancersResult": {
            "LoadBalancers": {
                "member": lbs
            },
            "NextMarker": null
        }
    }))
}

pub fn modify_load_balancer_attributes(
    state: &ElbState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "LoadBalancerArn")?;

    if !state.load_balancers.contains_key(arn) {
        return Err(resource_not_found("load balancer", arn));
    }

    // In a real implementation we'd store attributes; here we acknowledge.
    let attrs = input
        .get("Attributes")
        .cloned()
        .unwrap_or(json!([]));

    Ok(json!({
        "ModifyLoadBalancerAttributesResult": {
            "Attributes": attrs
        }
    }))
}
