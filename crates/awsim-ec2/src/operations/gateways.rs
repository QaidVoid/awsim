use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{invalid_parameter, resource_not_found},
    ids::new_ec2_id,
    state::{Ec2State, InternetGateway},
};

use super::require_str;

pub fn igw_to_value(igw: &InternetGateway) -> Value {
    let attachments: Vec<Value> = if let Some(vpc_id) = &igw.attached_vpc_id {
        vec![json!({ "vpcId": vpc_id, "state": "available" })]
    } else {
        vec![]
    };

    json!({
        "internetGatewayId": igw.internet_gateway_id,
        "attachmentSet": { "item": attachments },
    })
}

pub fn create_internet_gateway(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let internet_gateway_id = new_ec2_id("igw");

    let igw = InternetGateway {
        internet_gateway_id: internet_gateway_id.clone(),
        attached_vpc_id: None,
        tags: HashMap::new(),
    };

    let result = igw_to_value(&igw);
    state.internet_gateways.insert(internet_gateway_id, igw);

    Ok(json!({ "internetGateway": result }))
}

pub fn delete_internet_gateway(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let igw_id = require_str(input, "InternetGatewayId")?;

    {
        let igw = state
            .internet_gateways
            .get(igw_id)
            .ok_or_else(|| resource_not_found("internet gateway", igw_id))?;

        if igw.attached_vpc_id.is_some() {
            return Err(invalid_parameter(
                "The internet gateway has a VPC attachment. Detach it first.",
            ));
        }
    }

    state.internet_gateways.remove(igw_id);
    Ok(json!({}))
}

pub fn attach_internet_gateway(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let igw_id = require_str(input, "InternetGatewayId")?;
    let vpc_id = require_str(input, "VpcId")?;

    if !state.vpcs.contains_key(vpc_id) {
        return Err(resource_not_found("vpc", vpc_id));
    }

    let mut igw = state
        .internet_gateways
        .get_mut(igw_id)
        .ok_or_else(|| resource_not_found("internet gateway", igw_id))?;

    if igw.attached_vpc_id.is_some() {
        return Err(invalid_parameter(
            "The internet gateway is already attached to a VPC.",
        ));
    }

    igw.attached_vpc_id = Some(vpc_id.to_string());

    Ok(json!({}))
}

pub fn detach_internet_gateway(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let igw_id = require_str(input, "InternetGatewayId")?;
    let vpc_id = require_str(input, "VpcId")?;

    let mut igw = state
        .internet_gateways
        .get_mut(igw_id)
        .ok_or_else(|| resource_not_found("internet gateway", igw_id))?;

    match &igw.attached_vpc_id {
        Some(attached) if attached == vpc_id => {
            igw.attached_vpc_id = None;
        }
        _ => {
            return Err(invalid_parameter(format!(
                "The internet gateway '{igw_id}' is not attached to vpc '{vpc_id}'"
            )));
        }
    }

    Ok(json!({}))
}

pub fn describe_internet_gateways(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let igws: Vec<Value> = state
        .internet_gateways
        .iter()
        .map(|entry| igw_to_value(&entry))
        .collect();

    Ok(json!({ "internetGatewaySet": { "item": igws } }))
}
