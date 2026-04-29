use std::collections::HashMap;

use awsim_core::AwsError;
use serde_json::{Value, json};
use tracing::info;

use crate::{
    ids::{new_ec2_id, now_iso8601},
    state::{Ec2State, Instance, Subnet},
};

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

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
            "code": i.state_code(),
            "name": i.state,
        },
        "stateTransitionReason": i.state_transition_reason,
        "subnetId": i.subnet_id,
        "vpcId": i.vpc_id,
        "privateIpAddress": i.private_ip_address,
        "launchTime": i.launch_time,
        "reservationId": i.reservation_id,
        "tagSet": { "item": tags },
    })
}

/// Pull a list of `InstanceId.N` style entries out of a query-style input.
fn collect_id_param(input: &Value, key: &str) -> Vec<String> {
    match input.get(key) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(Value::Object(map)) => map
            .values()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// IP allocation — assigns from the subnet's CIDR base + a per-subnet cursor.
// ---------------------------------------------------------------------------

/// Pick the next available private IP for an instance launching in `subnet`.
/// Real EC2 uses host bits .4 onward; we start at .10 for a bit of headroom.
fn allocate_private_ip(state: &Ec2State, subnet: &Subnet) -> Option<String> {
    let base = parse_cidr_base(&subnet.cidr_block)?;
    let mut cursor = state
        .subnet_next_host
        .entry(subnet.subnet_id.clone())
        .or_insert(10);
    let host = *cursor;
    *cursor += 1;
    Some(host_to_ip(base, host))
}

/// Parse the network base from `a.b.c.d/N`, returning the 32-bit address with
/// the host bits zeroed.
fn parse_cidr_base(cidr: &str) -> Option<u32> {
    let (addr, mask) = cidr.split_once('/')?;
    let prefix: u32 = mask.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    let octets: Vec<&str> = addr.split('.').collect();
    if octets.len() != 4 {
        return None;
    }
    let mut acc = 0u32;
    for o in &octets {
        acc = (acc << 8) | o.parse::<u8>().ok()? as u32;
    }
    let mask_bits = if prefix == 0 {
        0
    } else {
        !0u32 << (32 - prefix)
    };
    Some(acc & mask_bits)
}

fn host_to_ip(base: u32, host: u32) -> String {
    let addr = base | host;
    format!(
        "{}.{}.{}.{}",
        (addr >> 24) & 0xff,
        (addr >> 16) & 0xff,
        (addr >> 8) & 0xff,
        addr & 0xff
    )
}

// ---------------------------------------------------------------------------
// RunInstances
// ---------------------------------------------------------------------------

/// Launch one or more EC2 instances inside a single reservation.
///
/// Diverges from real EC2 in that the lifecycle is synchronous — instances
/// land in `running` immediately. The state machine is otherwise honored
/// (Stop / Start / Reboot / Terminate transition cleanly), reservation
/// grouping matches AWS, and IPs are allocated from the launch subnet's
/// CIDR (defaulting to a fake 10.0.0.0/16 when no SubnetId is supplied).
pub fn run_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let image_id = input["ImageId"]
        .as_str()
        .unwrap_or("ami-00000000")
        .to_string();
    let instance_type = input["InstanceType"]
        .as_str()
        .unwrap_or("t2.micro")
        .to_string();
    let min_count = input["MinCount"].as_u64().unwrap_or(1);
    let max_count = input["MaxCount"].as_u64().unwrap_or(1);
    let count = min_count.max(1).min(max_count);

    let subnet_id = input["SubnetId"].as_str().map(|s| s.to_string());
    let now = now_iso8601();
    let reservation_id = new_ec2_id("r");

    let mut instances: Vec<Value> = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let instance_id = new_ec2_id("i");
        let (vpc_id, private_ip) = match subnet_id.as_ref() {
            Some(sid) => {
                let subnet = state.subnets.get(sid.as_str());
                let vpc = subnet.as_ref().map(|s| s.vpc_id.clone());
                let ip = subnet.and_then(|s| allocate_private_ip(state, &s));
                (vpc, ip)
            }
            None => (None, Some("10.0.0.10".to_string())),
        };

        let instance = Instance {
            instance_id: instance_id.clone(),
            instance_type: instance_type.clone(),
            image_id: image_id.clone(),
            state: "running".to_string(),
            previous_state: Some("pending".to_string()),
            state_transition_reason: String::new(),
            subnet_id: subnet_id.clone(),
            vpc_id,
            private_ip_address: private_ip,
            launch_time: now.clone(),
            reservation_id: reservation_id.clone(),
            tags: HashMap::new(),
        };

        let val = instance_to_value(&instance);
        info!(instance_id = %instance_id, reservation = %reservation_id, "RunInstances");
        state.instances.insert(instance_id, instance);
        instances.push(val);
    }

    Ok(json!({
        "instancesSet": { "item": instances },
        "reservationId": reservation_id,
        "ownerId": "000000000000",
    }))
}

// ---------------------------------------------------------------------------
// DescribeInstances — groups instances by reservation, supports the common
// instance-state-name filter that aws-cli / boto3 reach for first.
// ---------------------------------------------------------------------------

pub fn describe_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let id_filter = collect_id_param(input, "InstanceId");
    let state_filter = parse_state_filter(input);

    let mut by_reservation: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();

    for entry in state.instances.iter() {
        let inst = entry.value();
        if !id_filter.is_empty() && !id_filter.iter().any(|id| id == &inst.instance_id) {
            continue;
        }
        if !state_filter.is_empty() && !state_filter.iter().any(|s| s == &inst.state) {
            continue;
        }
        by_reservation
            .entry(inst.reservation_id.clone())
            .or_default()
            .push(instance_to_value(inst));
    }

    let reservations: Vec<Value> = by_reservation
        .into_iter()
        .map(|(reservation_id, items)| {
            json!({
                "reservationId": reservation_id,
                "ownerId": "000000000000",
                "instancesSet": { "item": items },
            })
        })
        .collect();

    Ok(json!({ "reservationSet": { "item": reservations } }))
}

/// Parse `Filter.N.{Name,Value.N}` into a flat list of values for the
/// `instance-state-name` filter only — that's the one tooling actually
/// uses to wait for `running` / `terminated`.
fn parse_state_filter(input: &Value) -> Vec<String> {
    let Some(filters) = input.get("Filter") else {
        return Vec::new();
    };
    let entries: Vec<&Value> = match filters {
        Value::Object(map) => map.values().collect(),
        Value::Array(arr) => arr.iter().collect(),
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries {
        let name = entry
            .get("Name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name != "instance-state-name" {
            continue;
        }
        if let Some(values) = entry.get("Value") {
            match values {
                Value::String(s) => out.push(s.clone()),
                Value::Object(map) => {
                    for v in map.values() {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                }
                Value::Array(arr) => {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// State-machine transitions: Start / Stop / Reboot / Terminate
// ---------------------------------------------------------------------------

/// Apply a transition guarded by the allowed predecessor states. Returns
/// the previous state name so callers can include it in the response.
fn transition(
    state: &Ec2State,
    instance_id: &str,
    target: &str,
    reason: &str,
    allowed_from: &[&str],
) -> Option<String> {
    let mut entry = state.instances.get_mut(instance_id)?;
    if !allowed_from.iter().any(|s| *s == entry.state) {
        return None;
    }
    let prev = entry.state.clone();
    entry.previous_state = Some(prev.clone());
    entry.state = target.to_string();
    entry.state_transition_reason = reason.to_string();
    Some(prev)
}

fn state_change(id: &str, prev: &str, current: &str) -> Value {
    let prev_code = code_for(prev);
    let cur_code = code_for(current);
    json!({
        "instanceId": id,
        "currentState": { "code": cur_code, "name": current },
        "previousState": { "code": prev_code, "name": prev },
    })
}

fn code_for(name: &str) -> u32 {
    match name {
        "pending" => 0,
        "running" => 16,
        "shutting-down" => 32,
        "terminated" => 48,
        "stopping" => 64,
        "stopped" => 80,
        _ => 0,
    }
}

/// StartInstances — only valid for `stopped` instances.
pub fn start_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let ids = collect_id_param(input, "InstanceId");
    let mut started: Vec<Value> = Vec::new();
    for id in &ids {
        if let Some(prev) = transition(state, id, "running", "User initiated", &["stopped"]) {
            started.push(state_change(id, &prev, "running"));
        }
    }
    Ok(json!({ "instancesSet": { "item": started } }))
}

/// StopInstances — `running` → `stopped` (real EC2 has a brief `stopping`
/// step; we collapse it for the same reason RunInstances skips `pending`).
pub fn stop_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let ids = collect_id_param(input, "InstanceId");
    let mut stopped: Vec<Value> = Vec::new();
    for id in &ids {
        if let Some(prev) = transition(state, id, "stopped", "User initiated", &["running"]) {
            stopped.push(state_change(id, &prev, "stopped"));
        }
    }
    Ok(json!({ "instancesSet": { "item": stopped } }))
}

/// RebootInstances — fire-and-forget; instance stays in `running`.
pub fn reboot_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let ids = collect_id_param(input, "InstanceId");
    for id in &ids {
        if let Some(mut inst) = state.instances.get_mut(id) {
            inst.state_transition_reason = "User initiated reboot".to_string();
        }
    }
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// TerminateInstances — moves to `terminated`. Real EC2 keeps the record
// queryable for ~1 hour; we do the same for the lifetime of the process so
// describe_instances after a terminate still surfaces them.
// ---------------------------------------------------------------------------

pub fn terminate_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let ids = collect_id_param(input, "InstanceId");
    let mut terminated: Vec<Value> = Vec::new();
    for id in &ids {
        if let Some(prev) = transition(
            state,
            id,
            "terminated",
            "Client.UserInitiatedShutdown",
            &["running", "stopped", "stopping", "pending"],
        ) {
            terminated.push(state_change(id, &prev, "terminated"));
        }
    }
    Ok(json!({ "instancesSet": { "item": terminated } }))
}

// ---------------------------------------------------------------------------
// DescribeInstanceStatus — surfaces the lifecycle state for non-terminated
// instances, matching what `aws ec2 wait instance-running` consumes.
// ---------------------------------------------------------------------------

pub fn describe_instance_status(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let id_filter = collect_id_param(input, "InstanceId");
    let include_all = input["IncludeAllInstances"].as_bool().unwrap_or(false);

    let items: Vec<Value> = state
        .instances
        .iter()
        .filter(|e| id_filter.is_empty() || id_filter.iter().any(|id| id == &e.instance_id))
        .filter(|e| {
            // Default behaviour mirrors AWS: only `running` shows up unless
            // the caller explicitly asks for everything.
            include_all || e.state == "running"
        })
        .map(|e| {
            let i = e.value();
            json!({
                "instanceId": i.instance_id,
                "availabilityZone": "us-east-1a",
                "instanceState": {
                    "code": i.state_code(),
                    "name": i.state,
                },
                "instanceStatus": { "status": "ok" },
                "systemStatus": { "status": "ok" },
            })
        })
        .collect();

    Ok(json!({ "instanceStatusSet": { "item": items } }))
}

// ---------------------------------------------------------------------------
// DescribeImages — small built-in catalog so listing isn't empty.
// ---------------------------------------------------------------------------

pub fn describe_images(_state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let images = vec![
        json!({
            "imageId": "ami-amazonlinux2",
            "name": "amzn2-ami-hvm-x86_64",
            "description": "Amazon Linux 2 AMI",
            "ownerId": "amazon",
            "platformDetails": "Linux/UNIX",
            "architecture": "x86_64",
            "rootDeviceType": "ebs",
            "virtualizationType": "hvm",
            "state": "available",
        }),
        json!({
            "imageId": "ami-ubuntu2204",
            "name": "ubuntu-jammy-22.04-amd64-server",
            "description": "Canonical Ubuntu 22.04 LTS",
            "ownerId": "099720109477",
            "platformDetails": "Linux/UNIX",
            "architecture": "x86_64",
            "rootDeviceType": "ebs",
            "virtualizationType": "hvm",
            "state": "available",
        }),
    ];
    Ok(json!({ "imagesSet": { "item": images } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_subnet(cidr: &str) -> (Ec2State, String) {
        let state = Ec2State::default();
        let subnet_id = "subnet-test".to_string();
        state.subnets.insert(
            subnet_id.clone(),
            Subnet {
                subnet_id: subnet_id.clone(),
                vpc_id: "vpc-test".to_string(),
                cidr_block: cidr.to_string(),
                availability_zone: "us-east-1a".to_string(),
                state: "available".to_string(),
                tags: HashMap::new(),
            },
        );
        (state, subnet_id)
    }

    #[test]
    fn run_instances_groups_into_one_reservation_and_assigns_distinct_ips() {
        let (state, subnet_id) = state_with_subnet("10.20.0.0/16");
        let resp = run_instances(
            &state,
            &json!({
                "ImageId": "ami-amazonlinux2",
                "InstanceType": "t3.small",
                "MinCount": 3,
                "MaxCount": 3,
                "SubnetId": subnet_id,
            }),
        )
        .unwrap();
        let reservation_id = resp["reservationId"].as_str().unwrap().to_string();
        let items = resp["instancesSet"]["item"].as_array().unwrap();
        assert_eq!(items.len(), 3);

        // Three distinct private IPs, all in 10.20.0.0/16, all share the
        // reservation id.
        let mut ips: Vec<String> = items
            .iter()
            .map(|i| i["privateIpAddress"].as_str().unwrap().to_string())
            .collect();
        ips.sort();
        ips.dedup();
        assert_eq!(ips.len(), 3);
        for ip in &ips {
            assert!(ip.starts_with("10.20."), "ip {ip} not in subnet CIDR");
        }
        for item in items {
            assert_eq!(item["reservationId"], json!(reservation_id));
        }
    }

    #[test]
    fn lifecycle_transitions_respect_predecessor_states() {
        let (state, subnet_id) = state_with_subnet("10.0.0.0/16");
        let resp = run_instances(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "SubnetId": subnet_id }),
        )
        .unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();

        // Stop only works from `running`.
        let stop = stop_instances(&state, &json!({ "InstanceId": id.clone() })).unwrap();
        assert_eq!(
            stop["instancesSet"]["item"][0]["currentState"]["name"],
            "stopped"
        );

        // Stop again is a no-op (current state is `stopped`, not in
        // `allowed_from`).
        let again = stop_instances(&state, &json!({ "InstanceId": id.clone() })).unwrap();
        assert!(again["instancesSet"]["item"].as_array().unwrap().is_empty());

        // Start moves it back to `running`.
        let start = start_instances(&state, &json!({ "InstanceId": id.clone() })).unwrap();
        assert_eq!(
            start["instancesSet"]["item"][0]["currentState"]["name"],
            "running"
        );

        // Terminate from `running` works; subsequent terminate is a no-op
        // because the instance is already terminated.
        let term = terminate_instances(&state, &json!({ "InstanceId": id.clone() })).unwrap();
        assert_eq!(
            term["instancesSet"]["item"][0]["currentState"]["name"],
            "terminated"
        );
        let term2 = terminate_instances(&state, &json!({ "InstanceId": id.clone() })).unwrap();
        assert!(term2["instancesSet"]["item"].as_array().unwrap().is_empty());
    }

    #[test]
    fn describe_instances_filters_by_state_name() {
        let (state, subnet_id) = state_with_subnet("10.0.0.0/16");
        let r1 = run_instances(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "SubnetId": subnet_id }),
        )
        .unwrap();
        let id1 = r1["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let r2 = run_instances(&state, &json!({ "MinCount": 1, "MaxCount": 1 })).unwrap();
        let id2 = r2["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        stop_instances(&state, &json!({ "InstanceId": id1.clone() })).unwrap();

        let resp = describe_instances(
            &state,
            &json!({
                "Filter": { "1": { "Name": "instance-state-name", "Value": { "1": "stopped" } } }
            }),
        )
        .unwrap();
        let mut got_ids: Vec<String> = resp["reservationSet"]["item"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| {
                r["instancesSet"]["item"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|i| i["instanceId"].as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .collect();
        got_ids.sort();
        assert_eq!(got_ids, vec![id1]);
        let _ = id2;
    }
}
