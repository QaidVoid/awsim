use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::new_ec2_id,
    state::{Ec2State, Subnet},
};

use super::require_str;

pub fn subnet_to_value(s: &Subnet) -> Value {
    json!({
        "subnetId": s.subnet_id,
        "vpcId": s.vpc_id,
        "cidrBlock": s.cidr_block,
        "availabilityZone": s.availability_zone,
        "state": s.state,
    })
}

pub fn create_subnet(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let vpc_id = require_str(input, "VpcId")?.to_string();
    let cidr_block = require_str(input, "CidrBlock")?.to_string();
    let availability_zone = input
        .get("AvailabilityZone")
        .and_then(|v| v.as_str())
        .unwrap_or("us-east-1a")
        .to_string();

    // Verify VPC exists
    if !state.vpcs.contains_key(&vpc_id) {
        return Err(resource_not_found("vpc", &vpc_id));
    }

    let subnet_id = new_ec2_id("subnet");
    let subnet = Subnet {
        subnet_id: subnet_id.clone(),
        vpc_id,
        cidr_block,
        availability_zone,
        state: "available".to_string(),
        tags: HashMap::new(),
    };

    let result = subnet_to_value(&subnet);
    state.subnets.insert(subnet_id, subnet);

    Ok(json!({ "subnet": result }))
}

pub fn delete_subnet(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let subnet_id = require_str(input, "SubnetId")?;

    if state.subnets.remove(subnet_id).is_none() {
        return Err(resource_not_found("subnet", subnet_id));
    }

    Ok(json!({}))
}

pub fn describe_subnets(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let subnet_id_filter: Vec<String> = {
        let mut ids = Vec::new();
        if let Some(obj) = input.get("SubnetId") {
            match obj {
                Value::String(s) => ids.push(s.clone()),
                Value::Array(arr) => {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            ids.push(s.to_string());
                        }
                    }
                }
                Value::Object(map) => {
                    for (_, v) in map {
                        if let Some(s) = v.as_str() {
                            ids.push(s.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        ids
    };

    let subnets: Vec<Value> = state
        .subnets
        .iter()
        .filter(|entry| {
            subnet_id_filter.is_empty()
                || subnet_id_filter.iter().any(|id| id == &entry.subnet_id)
        })
        .map(|entry| subnet_to_value(&entry))
        .collect();

    Ok(json!({ "subnetSet": { "item": subnets } }))
}
