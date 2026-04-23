use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};
use tracing::info;

use crate::{ids::{new_ec2_id, now_iso8601}, state::{Ec2State, Instance}};

fn instance_to_value(i: &Instance) -> Value {
    let tags: Vec<Value> = i
        .tags
        .iter()
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect();

    json!({
        "instanceId": i.instance_id,
        "instanceType": i.instance_type,
        "imageId": i.image_id,
        "instanceState": {
            "code": 16,
            "name": i.state,
        },
        "subnetId": i.subnet_id,
        "vpcId": i.vpc_id,
        "privateIpAddress": i.private_ip_address,
        "launchTime": i.launch_time,
        "tagSet": { "item": tags },
    })
}

// ---------------------------------------------------------------------------
// RunInstances
// ---------------------------------------------------------------------------

pub fn run_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let image_id = input["ImageId"].as_str().unwrap_or("ami-00000000").to_string();
    let instance_type = input["InstanceType"].as_str().unwrap_or("t2.micro").to_string();
    let min_count = input["MinCount"].as_u64().unwrap_or(1);
    let max_count = input["MaxCount"].as_u64().unwrap_or(1);
    let count = min_count.max(1).min(max_count);

    let subnet_id = input["SubnetId"].as_str().map(|s| s.to_string());
    let now = now_iso8601();

    let mut instances: Vec<Value> = Vec::new();

    for _ in 0..count {
        let instance_id = new_ec2_id("i");
        let vpc_id = subnet_id.as_ref().and_then(|sid| {
            state
                .subnets
                .get(sid.as_str())
                .map(|s| s.vpc_id.clone())
        });

        let instance = Instance {
            instance_id: instance_id.clone(),
            instance_type: instance_type.clone(),
            image_id: image_id.clone(),
            state: "running".to_string(),
            subnet_id: subnet_id.clone(),
            vpc_id,
            private_ip_address: Some("10.0.0.1".to_string()),
            launch_time: now.clone(),
            tags: HashMap::new(),
        };

        let val = instance_to_value(&instance);
        info!(instance_id = %instance_id, "RunInstances (stub)");
        state.instances.insert(instance_id, instance);
        instances.push(val);
    }

    Ok(json!({
        "instancesSet": { "item": instances },
        "reservationId": new_ec2_id("r"),
        "ownerId": "000000000000",
    }))
}

// ---------------------------------------------------------------------------
// DescribeInstances
// ---------------------------------------------------------------------------

pub fn describe_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    // Support InstanceId.1, InstanceId.2, ... filter
    let id_filter: Vec<String> = {
        let mut ids = Vec::new();
        if let Some(obj) = input.get("InstanceId") {
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

    let instances: Vec<Value> = state
        .instances
        .iter()
        .filter(|e| id_filter.is_empty() || id_filter.iter().any(|id| id == &e.instance_id))
        .map(|e| instance_to_value(&e))
        .collect();

    let reservations: Vec<Value> = instances
        .into_iter()
        .map(|i| {
            json!({
                "reservationId": new_ec2_id("r"),
                "ownerId": "000000000000",
                "instancesSet": { "item": [i] },
            })
        })
        .collect();

    Ok(json!({ "reservationSet": { "item": reservations } }))
}

// ---------------------------------------------------------------------------
// TerminateInstances
// ---------------------------------------------------------------------------

pub fn terminate_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let id_iter: Vec<String> = match input.get("InstanceId") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::Object(map)) => map
            .values()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    };

    let mut terminated: Vec<Value> = Vec::new();
    for id in &id_iter {
        if let Some((_, _)) = state.instances.remove(id.as_str()) {
            terminated.push(json!({
                "instanceId": id,
                "currentState": { "code": 48, "name": "terminated" },
                "previousState": { "code": 16, "name": "running" },
            }));
        }
    }

    Ok(json!({ "instancesSet": { "item": terminated } }))
}

// ---------------------------------------------------------------------------
// DescribeInstanceStatus
// ---------------------------------------------------------------------------

pub fn describe_instance_status(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "instanceStatusSet": {} }))
}

// ---------------------------------------------------------------------------
// DescribeImages (AMI listing stub)
// ---------------------------------------------------------------------------

pub fn describe_images(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    Ok(json!({ "imagesSet": {} }))
}
