use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::{invalid_parameter, resource_not_found},
    ids::new_ec2_id,
    state::{Ec2State, Route, RouteTable},
};

use super::{opt_str, require_str};

pub fn route_table_to_value(rt: &RouteTable) -> Value {
    let routes: Vec<Value> = rt
        .routes
        .iter()
        .map(|r| {
            json!({
                "destinationCidrBlock": r.destination_cidr_block,
                "gatewayId": r.gateway_id,
                "state": r.state,
            })
        })
        .collect();

    let associations: Vec<Value> = rt
        .associations
        .iter()
        .map(|(subnet_id, assoc_id)| {
            json!({
                "routeTableAssociationId": assoc_id,
                "routeTableId": rt.route_table_id,
                "subnetId": subnet_id,
            })
        })
        .collect();

    json!({
        "routeTableId": rt.route_table_id,
        "vpcId": rt.vpc_id,
        "routeSet": { "item": routes },
        "associationSet": { "item": associations },
    })
}

pub fn create_route_table(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let vpc_id = require_str(input, "VpcId")?.to_string();

    if !state.vpcs.contains_key(&vpc_id) {
        return Err(resource_not_found("vpc", &vpc_id));
    }

    let route_table_id = new_ec2_id("rtb");

    // Default local route
    let local_route = Route {
        destination_cidr_block: "local".to_string(),
        gateway_id: Some("local".to_string()),
        state: "active".to_string(),
    };

    let rt = RouteTable {
        route_table_id: route_table_id.clone(),
        vpc_id,
        routes: vec![local_route],
        associations: HashMap::new(),
        tags: HashMap::new(),
    };

    let result = route_table_to_value(&rt);
    state.route_tables.insert(route_table_id, rt);

    Ok(json!({ "routeTable": result }))
}

pub fn delete_route_table(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let route_table_id = require_str(input, "RouteTableId")?;

    if state.route_tables.remove(route_table_id).is_none() {
        return Err(resource_not_found("route table", route_table_id));
    }

    Ok(json!({}))
}

pub fn describe_route_tables(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let rts: Vec<Value> = state
        .route_tables
        .iter()
        .map(|entry| route_table_to_value(&entry))
        .collect();

    Ok(json!({ "routeTableSet": { "item": rts } }))
}

pub fn create_route(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let route_table_id = require_str(input, "RouteTableId")?;
    let destination_cidr_block = require_str(input, "DestinationCidrBlock")?.to_string();
    let gateway_id = opt_str(input, "GatewayId").map(|s| s.to_string());

    let mut rt = state
        .route_tables
        .get_mut(route_table_id)
        .ok_or_else(|| resource_not_found("route table", route_table_id))?;

    // Check for duplicate destination
    if rt
        .routes
        .iter()
        .any(|r| r.destination_cidr_block == destination_cidr_block)
    {
        return Err(invalid_parameter(format!(
            "Route with destination '{destination_cidr_block}' already exists"
        )));
    }

    rt.routes.push(Route {
        destination_cidr_block,
        gateway_id,
        state: "active".to_string(),
    });

    Ok(json!({ "return": true }))
}

pub fn associate_route_table(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let route_table_id = require_str(input, "RouteTableId")?;
    let subnet_id = require_str(input, "SubnetId")?.to_string();

    if !state.subnets.contains_key(&subnet_id) {
        return Err(resource_not_found("subnet", &subnet_id));
    }

    let mut rt = state
        .route_tables
        .get_mut(route_table_id)
        .ok_or_else(|| resource_not_found("route table", route_table_id))?;

    let association_id = new_ec2_id("rtbassoc");
    rt.associations.insert(subnet_id, association_id.clone());

    Ok(json!({ "associationId": association_id }))
}
