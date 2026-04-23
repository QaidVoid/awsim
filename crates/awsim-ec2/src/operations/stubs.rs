use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::Ec2State;

// ---------------------------------------------------------------------------
// DescribeNetworkInterfaces
// ---------------------------------------------------------------------------

pub fn describe_network_interfaces(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "networkInterfaceSet": {} }))
}

// ---------------------------------------------------------------------------
// DescribeNatGateways
// ---------------------------------------------------------------------------

pub fn describe_nat_gateways(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "natGatewaySet": {} }))
}

// ---------------------------------------------------------------------------
// DescribeVpcEndpoints
// ---------------------------------------------------------------------------

pub fn describe_vpc_endpoints(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "vpcEndpointSet": {} }))
}

// ---------------------------------------------------------------------------
// DescribeAddresses
// ---------------------------------------------------------------------------

pub fn describe_addresses(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let addresses: Vec<Value> = state
        .addresses
        .iter()
        .map(|e| {
            json!({
                "allocationId": e.allocation_id,
                "publicIp": e.public_ip,
                "instanceId": e.instance_id,
                "domain": "vpc",
            })
        })
        .collect();

    Ok(json!({ "addressesSet": { "item": addresses } }))
}
