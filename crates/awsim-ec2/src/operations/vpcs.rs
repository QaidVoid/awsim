use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::new_ec2_id,
    state::{Ec2State, Vpc},
};

use super::require_str;

pub fn vpc_to_value(v: &Vpc) -> Value {
    json!({
        "vpcId": v.vpc_id,
        "cidrBlock": v.cidr_block,
        "state": v.state,
        "isDefault": v.is_default,
    })
}

pub fn create_vpc(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let cidr_block = require_str(input, "CidrBlock")?.to_string();
    let vpc_id = new_ec2_id("vpc");

    let vpc = Vpc {
        vpc_id: vpc_id.clone(),
        cidr_block,
        state: "available".to_string(),
        is_default: false,
        tags: HashMap::new(),
    };

    let result = vpc_to_value(&vpc);
    state.vpcs.insert(vpc_id, vpc);

    Ok(json!({ "vpc": result }))
}

pub fn delete_vpc(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let vpc_id = require_str(input, "VpcId")?;

    if state.vpcs.remove(vpc_id).is_none() {
        return Err(resource_not_found("vpc", vpc_id));
    }

    Ok(json!({}))
}

pub fn describe_vpcs(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    // Support VpcId.1, VpcId.2, ... or VpcId as a flat array
    let vpc_id_filter: Vec<String> = {
        let mut ids = Vec::new();
        if let Some(obj) = input.get("VpcId") {
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
                    // Handles VpcId.1, VpcId.2 etc. (parsed as an object/array by query parser)
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

    let vpcs: Vec<Value> = state
        .vpcs
        .iter()
        .filter(|entry| {
            vpc_id_filter.is_empty() || vpc_id_filter.iter().any(|id| id == &entry.vpc_id)
        })
        .map(|entry| {
            vpc_to_value(&entry)
        })
        .collect();

    Ok(json!({ "vpcSet": { "item": vpcs } }))
}
