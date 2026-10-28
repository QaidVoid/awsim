use awsim_core::tags::{TagOpts, validate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::operations::filters::{matches_all, parse_filters, tag_field};
use crate::{
    ids::{new_ec2_id, now_iso8601},
    state::{BlockDevice, Ec2State, Instance, Subnet, Volume, VolumeAttachment},
};

/// Root volume size, in GiB, when a launch does not state one. Matches
/// the 8 GiB root volume of the common Linux AMIs.
pub const DEFAULT_ROOT_GIB: u64 = 8;

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

pub(crate) fn instance_to_value(i: &Instance) -> Value {
    let tags: Vec<Value> = i
        .tags
        .iter()
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect();
    let block_devices: Vec<Value> = i
        .block_devices
        .iter()
        .map(|d| {
            json!({
                "deviceName": d.device_name,
                "ebs": {
                    "volumeId": d.volume_id,
                    "status": "attached",
                    "attachTime": d.attach_time,
                    "deleteOnTermination": d.delete_on_termination,
                },
            })
        })
        .collect();
    let groups: Vec<Value> = i
        .security_group_ids
        .iter()
        .map(|g| json!({ "groupId": g }))
        .collect();

    let mut value = json!({
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
        "placement": { "availabilityZone": i.availability_zone },
        "rootDeviceName": i.root_device_name,
        "rootDeviceType": "ebs",
        "blockDeviceMapping": { "item": block_devices },
        "groupSet": { "item": groups },
        "tagSet": { "item": tags },
        // Enhanced networking flags: AWS turns these on for the
        // modern instance families that support them. Real AWS sets
        // EnaSupport=true on Nitro-era types and `SriovNetSupport`
        // (a string, not a bool) to `"simple"` on any instance that
        // supports the older Intel 82599 VF driver. We treat the
        // synthetic instances as Nitro by default so SDKs that key
        // off the flag see the modern value.
        "enaSupport": true,
        "sriovNetSupport": "simple",
    });
    if let Some(arn) = &i.iam_instance_profile {
        value["iamInstanceProfile"] = json!({ "arn": arn });
    }
    if let Some(token) = &i.client_token {
        value["clientToken"] = json!(token);
    }
    value
}

/// Pull a list of `InstanceId.N` style entries out of a query-style input.
pub(crate) fn collect_id_param(input: &Value, key: &str) -> Vec<String> {
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
// IP allocation. Assigns from the subnet's CIDR base + a per-subnet cursor.
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

/// What a `RunInstances` request asks for, parsed once for both backends.
pub(crate) struct LaunchRequest {
    pub image_id: String,
    pub instance_type: String,
    pub count: u64,
    pub subnet_id: Option<String>,
    pub security_group_ids: Vec<String>,
    pub user_data: Option<String>,
    pub iam_instance_profile: Option<String>,
    pub client_token: Option<String>,
    pub root_device_name: String,
    pub root_size_gib: u64,
    pub root_volume_type: String,
    pub instance_tags: Vec<(String, String)>,
    pub volume_tags: Vec<(String, String)>,
}

impl LaunchRequest {
    pub(crate) fn parse(input: &Value, ctx: &RequestContext) -> Result<Self, AwsError> {
        let min_count = input["MinCount"]
            .as_u64()
            .or_else(|| str_u64(&input["MinCount"]));
        let max_count = input["MaxCount"]
            .as_u64()
            .or_else(|| str_u64(&input["MaxCount"]));
        let (min_count, max_count) = (min_count.unwrap_or(1), max_count.unwrap_or(1));
        // AWS rejects MinCount of 0 with InvalidParameterValue, and a
        // MinCount that exceeds MaxCount with InvalidParameterCombination.
        // Without these checks the silent-clamp on min_count.max(1) hid
        // the misconfiguration so a caller's "give me at least 5" turned
        // into "give me 1" instead of the real-AWS error.
        if min_count == 0 {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                "MinCount must be at least 1.",
            ));
        }
        if max_count == 0 {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                "MaxCount must be at least 1.",
            ));
        }
        if min_count > max_count {
            return Err(AwsError::bad_request(
                "InvalidParameterCombination",
                format!(
                    "MinCount ({min_count}) must be less than or equal to MaxCount ({max_count})."
                ),
            ));
        }

        // The SDKs send the subnet and groups either at the top level or
        // on the first network interface; the interface wins when both
        // are present, as it does on EC2.
        let nic = input
            .get("NetworkInterface")
            .and_then(|n| n.as_array())
            .and_then(|n| n.first());
        let subnet_id = nic
            .and_then(|n| n["SubnetId"].as_str())
            .or_else(|| input["SubnetId"].as_str())
            .map(str::to_string);
        let mut security_group_ids = nic
            .map(|n| collect_id_param(n, "SecurityGroupId"))
            .unwrap_or_default();
        if security_group_ids.is_empty() {
            security_group_ids = collect_id_param(input, "SecurityGroupId");
        }

        let profile = &input["IamInstanceProfile"];
        let iam_instance_profile = profile["Arn"].as_str().map(str::to_string).or_else(|| {
            profile["Name"].as_str().map(|name| {
                format!(
                    "arn:{}:iam::{}:instance-profile/{name}",
                    ctx.partition, ctx.account_id
                )
            })
        });

        let mut root_device_name = "/dev/xvda".to_string();
        let mut root_size_gib = DEFAULT_ROOT_GIB;
        let mut root_volume_type = "gp3".to_string();
        if let Some(mapping) = input["BlockDeviceMapping"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m.get("Ebs").is_some()))
        {
            if let Some(device) = mapping["DeviceName"].as_str() {
                root_device_name = device.to_string();
            }
            let ebs = &mapping["Ebs"];
            if let Some(size) = ebs["VolumeSize"]
                .as_u64()
                .or_else(|| str_u64(&ebs["VolumeSize"]))
            {
                if size == 0 {
                    return Err(AwsError::bad_request(
                        "InvalidParameterValue",
                        "Volume of size 0GB is smaller than snapshot, expect size >= 1GB",
                    ));
                }
                root_size_gib = size;
            }
            if let Some(kind) = ebs["VolumeType"].as_str() {
                root_volume_type = kind.to_string();
            }
        }

        let mut instance_tags = Vec::new();
        let mut volume_tags = Vec::new();
        for spec in input["TagSpecification"].as_array().into_iter().flatten() {
            let tags: Vec<(String, String)> = spec["Tag"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    Some((
                        t["Key"].as_str()?.to_string(),
                        t["Value"].as_str().unwrap_or_default().to_string(),
                    ))
                })
                .collect();
            validate(&tags, &TagOpts::aws_default())?;
            match spec["ResourceType"].as_str() {
                Some("instance") => instance_tags.extend(tags),
                Some("volume") => volume_tags.extend(tags),
                _ => {}
            }
        }

        Ok(Self {
            image_id: input["ImageId"]
                .as_str()
                .unwrap_or("ami-00000000")
                .to_string(),
            instance_type: input["InstanceType"]
                .as_str()
                .unwrap_or("t2.micro")
                .to_string(),
            count: max_count,
            subnet_id,
            security_group_ids,
            user_data: input["UserData"].as_str().map(str::to_string),
            iam_instance_profile,
            client_token: input["ClientToken"].as_str().map(str::to_string),
            root_device_name,
            root_size_gib,
            root_volume_type,
            instance_tags,
            volume_tags,
        })
    }
}

fn str_u64(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok())
}

/// The instances an earlier launch with this client token created, as
/// that launch's response. EC2 answers a repeated token with the original
/// reservation rather than launching again.
pub(crate) fn launched_with_token(state: &Ec2State, token: &str) -> Option<Value> {
    let mut found: Vec<Instance> = state
        .instances
        .iter()
        .filter(|e| e.client_token.as_deref() == Some(token))
        .map(|e| e.value().clone())
        .collect();
    if found.is_empty() {
        return None;
    }
    found.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    let reservation_id = found[0].reservation_id.clone();
    let items: Vec<Value> = found.iter().map(instance_to_value).collect();
    Some(json!({
        "instancesSet": { "item": items },
        "reservationId": reservation_id,
        "ownerId": "000000000000",
    }))
}

/// Build one instance of a launch and record its root volume. The caller
/// inserts the instance.
pub(crate) fn new_instance(
    state: &Ec2State,
    launch: &LaunchRequest,
    reservation_id: &str,
    initial_state: &str,
    ctx: &RequestContext,
) -> Instance {
    let instance_id = new_ec2_id("i");
    let now = now_iso8601();
    let subnet = launch
        .subnet_id
        .as_ref()
        .and_then(|sid| state.subnets.get(sid.as_str()).map(|s| s.value().clone()));
    let (vpc_id, private_ip) = match (&launch.subnet_id, &subnet) {
        (Some(_), Some(subnet)) => (
            Some(subnet.vpc_id.clone()),
            allocate_private_ip(state, subnet),
        ),
        (Some(_), None) => (None, None),
        (None, _) => (None, Some("10.0.0.10".to_string())),
    };
    let availability_zone = subnet
        .as_ref()
        .map(|s| s.availability_zone.clone())
        .unwrap_or_else(|| format!("{}a", ctx.region));

    let root_volume_id = new_ec2_id("vol");
    state.volumes.insert(
        root_volume_id.clone(),
        Volume {
            volume_id: root_volume_id.clone(),
            size_gib: launch.root_size_gib,
            volume_type: launch.root_volume_type.clone(),
            availability_zone: availability_zone.clone(),
            state: "in-use".to_string(),
            create_time: now.clone(),
            encrypted: false,
            kms_key_id: None,
            iops: None,
            throughput: None,
            client_token: None,
            attachment: Some(VolumeAttachment {
                instance_id: instance_id.clone(),
                device: launch.root_device_name.clone(),
                attach_time: now.clone(),
                delete_on_termination: true,
            }),
            tags: launch.volume_tags.iter().cloned().collect(),
        },
    );
    record_tags(state, &root_volume_id, &launch.volume_tags);
    record_tags(state, &instance_id, &launch.instance_tags);

    Instance {
        instance_id: instance_id.clone(),
        instance_type: launch.instance_type.clone(),
        image_id: launch.image_id.clone(),
        state: initial_state.to_string(),
        previous_state: Some("pending".to_string()),
        state_transition_reason: String::new(),
        subnet_id: launch.subnet_id.clone(),
        vpc_id,
        private_ip_address: private_ip,
        launch_time: now.clone(),
        reservation_id: reservation_id.to_string(),
        tags: launch.instance_tags.iter().cloned().collect(),
        user_data: launch.user_data.clone(),
        availability_zone,
        security_group_ids: launch.security_group_ids.clone(),
        iam_instance_profile: launch.iam_instance_profile.clone(),
        client_token: launch.client_token.clone(),
        root_device_name: launch.root_device_name.clone(),
        block_devices: vec![BlockDevice {
            device_name: launch.root_device_name.clone(),
            volume_id: root_volume_id,
            attach_time: now,
            delete_on_termination: true,
        }],
        vm: false,
    }
}

/// Keep `DescribeTags` in step with tags applied at creation.
pub(crate) fn record_tags(state: &Ec2State, resource_id: &str, tags: &[(String, String)]) {
    if tags.is_empty() {
        return;
    }
    let mut entry = state
        .resource_tags
        .entry(resource_id.to_string())
        .or_default();
    for (k, v) in tags {
        entry.insert(k.clone(), v.clone());
    }
}

/// Launch one or more EC2 instances inside a single reservation.
///
/// Without a VM backend the lifecycle is synchronous: instances land in
/// `running` immediately. The state machine is otherwise honored
/// (Stop / Start / Reboot / Terminate transition cleanly), reservation
/// grouping matches AWS, and IPs are allocated from the launch subnet's
/// CIDR (defaulting to a fake 10.0.0.0/16 when no SubnetId is supplied).
pub fn run_instances(
    state: &Ec2State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let launch = LaunchRequest::parse(input, ctx)?;
    if let Some(token) = &launch.client_token
        && let Some(previous) = launched_with_token(state, token)
    {
        return Ok(previous);
    }
    let reservation_id = new_ec2_id("r");

    let mut instances: Vec<Value> = Vec::with_capacity(launch.count as usize);
    for _ in 0..launch.count {
        let instance = new_instance(state, &launch, &reservation_id, "running", ctx);
        let val = instance_to_value(&instance);
        info!(instance_id = %instance.instance_id, reservation = %reservation_id, "RunInstances");
        state
            .instances
            .insert(instance.instance_id.clone(), instance);
        instances.push(val);
    }

    Ok(json!({
        "instancesSet": { "item": instances },
        "reservationId": reservation_id,
        "ownerId": "000000000000",
    }))
}

// ---------------------------------------------------------------------------
// DescribeInstances. Groups instances by reservation, and supports the
// filters tooling reaches for first: state, id and tags.
// ---------------------------------------------------------------------------

pub fn describe_instances(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let id_filter = collect_id_param(input, "InstanceId");
    let filters = parse_filters(input);

    let mut by_reservation: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();

    for entry in state.instances.iter() {
        let inst = entry.value();
        if !id_filter.is_empty() && !id_filter.iter().any(|id| id == &inst.instance_id) {
            continue;
        }
        let pass = matches_all(&filters, |name| match name {
            "instance-state-name" => Some(vec![inst.state.clone()]),
            "instance-id" => Some(vec![inst.instance_id.clone()]),
            "instance-type" => Some(vec![inst.instance_type.clone()]),
            "subnet-id" => Some(inst.subnet_id.clone().into_iter().collect()),
            "client-token" => Some(inst.client_token.clone().into_iter().collect()),
            _ => tag_field(&inst.tags, name),
        });
        if !pass {
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

/// StartInstances. Only valid for `stopped` instances.
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

/// StopInstances. `running` -> `stopped` (real EC2 has a brief `stopping`
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

/// RebootInstances. Fire-and-forget; instance stays in `running`.
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
// TerminateInstances. Moves to `terminated`. Real EC2 keeps the record
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
            release_volumes(state, id);
            terminated.push(state_change(id, &prev, "terminated"));
        }
    }
    Ok(json!({ "instancesSet": { "item": terminated } }))
}

/// Settle a terminated instance's volumes as EC2 does: delete the ones
/// marked delete-on-termination and detach the rest. Returns the ids of
/// the volumes that were detached and kept.
pub(crate) fn release_volumes(state: &Ec2State, instance_id: &str) -> Vec<String> {
    let devices = match state.instances.get_mut(instance_id) {
        Some(mut inst) => std::mem::take(&mut inst.block_devices),
        None => return Vec::new(),
    };
    let mut kept = Vec::new();
    for device in devices {
        if device.delete_on_termination {
            state.volumes.remove(&device.volume_id);
            state.resource_tags.remove(&device.volume_id);
        } else if let Some(mut vol) = state.volumes.get_mut(&device.volume_id) {
            vol.state = "available".to_string();
            vol.attachment = None;
            kept.push(device.volume_id);
        }
    }
    kept
}

// ---------------------------------------------------------------------------
// DescribeInstanceAttribute. Surfaces a single instance attribute. AWS
// returns each attribute under a top-level key matching its name; the
// caller selects exactly one via the `Attribute` parameter.
// ---------------------------------------------------------------------------

pub fn describe_instance_attribute(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "InstanceId is required."))?;
    let attribute = input["Attribute"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Attribute is required."))?;
    let instance = state.instances.get(instance_id).ok_or_else(|| {
        AwsError::not_found(
            "InvalidInstanceID.NotFound",
            format!("Instance not found: {instance_id}"),
        )
    })?;

    let mut response = json!({ "instanceId": instance.instance_id });
    match attribute {
        "userData" => {
            response["userData"] = match &instance.user_data {
                Some(s) => json!({ "value": s }),
                None => Value::Null,
            };
        }
        "instanceType" => {
            response["instanceType"] = json!({ "value": instance.instance_type });
        }
        "disableApiTermination" => {
            response["disableApiTermination"] = json!({ "value": false });
        }
        "rootDeviceName" => {
            response["rootDeviceName"] = json!({ "value": "/dev/xvda" });
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!("Unsupported Attribute `{other}`."),
            ));
        }
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// ModifyInstanceAttribute. The instance type and user data, both of which
// EC2 only lets change while the instance is stopped.
// ---------------------------------------------------------------------------

/// A validated `ModifyInstanceAttribute`, not yet applied.
pub(crate) enum Modification {
    InstanceType { instance_id: String, value: String },
    UserData { instance_id: String, value: String },
}

pub(crate) fn plan_modify_instance_attribute(
    state: &Ec2State,
    input: &Value,
) -> Result<Modification, AwsError> {
    let instance_id = crate::operations::require_str(input, "InstanceId")?;
    let instance = state.instances.get(instance_id).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidInstanceID.NotFound",
            format!("The instance ID '{instance_id}' does not exist"),
        )
    })?;
    // Both shapes are on the wire: `InstanceType.Value=x`, and the older
    // `Attribute=instanceType&Value=x`.
    let attribute = input["Attribute"].as_str();
    let plain = input["Value"].as_str();
    let instance_type = input["InstanceType"]["Value"]
        .as_str()
        .or(plain.filter(|_| attribute == Some("instanceType")));
    let user_data = input["UserData"]["Value"]
        .as_str()
        .or(plain.filter(|_| attribute == Some("userData")));

    let modification = match (instance_type, user_data) {
        (Some(value), None) => Modification::InstanceType {
            instance_id: instance_id.to_string(),
            value: value.to_string(),
        },
        (None, Some(value)) => Modification::UserData {
            instance_id: instance_id.to_string(),
            value: value.to_string(),
        },
        (Some(_), Some(_)) => {
            return Err(AwsError::bad_request(
                "InvalidParameterCombination",
                "Only one attribute can be modified at a time",
            ));
        }
        (None, None) => {
            return Err(AwsError::not_implemented("ModifyInstanceAttribute"));
        }
    };
    if instance.state != "stopped" {
        return Err(AwsError::bad_request(
            "IncorrectInstanceState",
            format!("The instance '{instance_id}' is not in the 'stopped' state."),
        ));
    }
    if let Modification::InstanceType { value, .. } = &modification
        && !value.contains('.')
    {
        return Err(AwsError::bad_request(
            "InvalidInstanceAttributeValue",
            format!("The instanceType '{value}' is not supported."),
        ));
    }
    Ok(modification)
}

pub(crate) fn commit_modify_instance_attribute(state: &Ec2State, modification: &Modification) {
    match modification {
        Modification::InstanceType { instance_id, value } => {
            if let Some(mut instance) = state.instances.get_mut(instance_id) {
                instance.instance_type = value.clone();
            }
        }
        Modification::UserData { instance_id, value } => {
            if let Some(mut instance) = state.instances.get_mut(instance_id) {
                instance.user_data = Some(value.clone());
            }
        }
    }
}

pub fn modify_instance_attribute(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let modification = plan_modify_instance_attribute(state, input)?;
    commit_modify_instance_attribute(state, &modification);
    Ok(json!({ "return": true }))
}

// ---------------------------------------------------------------------------
// DescribeInstanceStatus. Surfaces the lifecycle state for non-terminated
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
// DescribeImages. Small built-in catalog so listing isn't empty.
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
    use std::collections::HashMap;

    use super::*;

    fn run(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
        run_instances(state, input, &RequestContext::new("ec2", "us-east-1"))
    }

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
        let resp = run(
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
        let resp = run(
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
        let r1 = run(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "SubnetId": subnet_id }),
        )
        .unwrap();
        let id1 = r1["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let r2 = run(&state, &json!({ "MinCount": 1, "MaxCount": 1 })).unwrap();
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

    #[test]
    fn run_instances_persists_user_data_for_describe_instance_attribute() {
        let (state, subnet_id) = state_with_subnet("10.0.0.0/16");
        let resp = run(
            &state,
            &json!({
                "MinCount": 1,
                "MaxCount": 1,
                "SubnetId": subnet_id,
                "UserData": "IyEvYmluL2Jhc2gKZWNobyBoaQ==",
            }),
        )
        .unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();

        let attr = describe_instance_attribute(
            &state,
            &json!({ "InstanceId": id, "Attribute": "userData" }),
        )
        .unwrap();
        assert_eq!(
            attr["userData"]["value"],
            json!("IyEvYmluL2Jhc2gKZWNobyBoaQ==")
        );
    }

    #[test]
    fn describe_instance_attribute_returns_null_when_user_data_absent() {
        let (state, subnet_id) = state_with_subnet("10.0.0.0/16");
        let resp = run(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "SubnetId": subnet_id }),
        )
        .unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let attr = describe_instance_attribute(
            &state,
            &json!({ "InstanceId": id, "Attribute": "userData" }),
        )
        .unwrap();
        assert!(attr["userData"].is_null());
    }

    #[test]
    fn describe_instance_attribute_rejects_unknown_attribute() {
        let (state, subnet_id) = state_with_subnet("10.0.0.0/16");
        let resp = run(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "SubnetId": subnet_id }),
        )
        .unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let err =
            describe_instance_attribute(&state, &json!({ "InstanceId": id, "Attribute": "magic" }))
                .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn run_instances_surfaces_ena_and_sriov_flags() {
        let state = Ec2State::default();
        let resp = run(
            &state,
            &json!({ "ImageId": "ami-12345678", "MinCount": 1, "MaxCount": 1 }),
        )
        .unwrap();
        let inst = &resp["instancesSet"]["item"][0];
        assert_eq!(inst["enaSupport"], true);
        assert_eq!(inst["sriovNetSupport"], "simple");
    }

    fn workspace_launch(subnet_id: &str, token: &str) -> Value {
        json!({
            "ImageId": "ami-ws",
            "InstanceType": "t3.large",
            "MinCount": "1",
            "MaxCount": "1",
            "ClientToken": token,
            "NetworkInterface": [{
                "DeviceIndex": "0",
                "SubnetId": subnet_id,
                "SecurityGroupId": ["sg-1", "sg-2"],
                "AssociatePublicIpAddress": "false",
            }],
            "IamInstanceProfile": { "Arn": "arn:aws:iam::000000000000:instance-profile/p/ws" },
            "BlockDeviceMapping": [{
                "DeviceName": "/dev/xvda",
                "Ebs": { "VolumeSize": "30", "DeleteOnTermination": "true" },
            }],
            "TagSpecification": [
                { "ResourceType": "instance", "Tag": [{ "Key": "app:tenant", "Value": "t1" }] },
                { "ResourceType": "volume", "Tag": [{ "Key": "app:tenant", "Value": "t1" }] },
            ],
        })
    }

    #[test]
    fn run_instances_honours_network_interface_tags_profile_and_root_size() {
        let (state, subnet_id) = state_with_subnet("10.1.0.0/16");
        let resp = run(&state, &workspace_launch(&subnet_id, "tok-1")).unwrap();
        let inst = &resp["instancesSet"]["item"][0];
        assert_eq!(inst["subnetId"], json!(subnet_id));
        assert_eq!(inst["vpcId"], "vpc-test");
        assert_eq!(inst["groupSet"]["item"][1]["groupId"], "sg-2");
        assert_eq!(
            inst["iamInstanceProfile"]["arn"],
            "arn:aws:iam::000000000000:instance-profile/p/ws"
        );
        assert_eq!(inst["tagSet"]["item"][0]["key"], "app:tenant");
        assert_eq!(inst["rootDeviceName"], "/dev/xvda");
        let root = &inst["blockDeviceMapping"]["item"][0];
        assert_eq!(root["deviceName"], "/dev/xvda");
        let volume_id = root["ebs"]["volumeId"].as_str().unwrap();
        let volume = state.volumes.get(volume_id).unwrap();
        assert_eq!(volume.size_gib, 30);
        assert_eq!(volume.state, "in-use");
        assert_eq!(volume.tags.get("app:tenant").unwrap(), "t1");
    }

    #[test]
    fn run_instances_names_profile_by_name() {
        let state = Ec2State::default();
        let resp = run(
            &state,
            &json!({ "MinCount": 1, "MaxCount": 1, "IamInstanceProfile": { "Name": "ws" } }),
        )
        .unwrap();
        assert_eq!(
            resp["instancesSet"]["item"][0]["iamInstanceProfile"]["arn"],
            "arn:aws:iam::000000000000:instance-profile/ws"
        );
    }

    #[test]
    fn run_instances_repeated_client_token_returns_first_launch() {
        let (state, subnet_id) = state_with_subnet("10.1.0.0/16");
        let first = run(&state, &workspace_launch(&subnet_id, "tok-1")).unwrap();
        let again = run(&state, &workspace_launch(&subnet_id, "tok-1")).unwrap();
        assert_eq!(
            first["instancesSet"]["item"][0]["instanceId"],
            again["instancesSet"]["item"][0]["instanceId"]
        );
        assert_eq!(first["reservationId"], again["reservationId"]);
        assert_eq!(state.instances.len(), 1);
        assert_eq!(state.volumes.len(), 1);
    }

    #[test]
    fn describe_instances_filters_by_tag() {
        let (state, subnet_id) = state_with_subnet("10.1.0.0/16");
        run(&state, &workspace_launch(&subnet_id, "tok-1")).unwrap();
        run(&state, &json!({ "MinCount": 1, "MaxCount": 1 })).unwrap();
        let count = |value: &str| {
            let resp = describe_instances(
                &state,
                &json!({ "Filter": [{ "Name": "tag:app:tenant", "Value": [value] }] }),
            )
            .unwrap();
            resp["reservationSet"]["item"].as_array().unwrap().len()
        };
        assert_eq!(count("t1"), 1);
        assert_eq!(count("t2"), 0);
    }

    #[test]
    fn terminate_deletes_root_volume_and_detaches_others() {
        let (state, subnet_id) = state_with_subnet("10.1.0.0/16");
        let resp = run(&state, &workspace_launch(&subnet_id, "tok-1")).unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let root = state.instances.get(&id).unwrap().block_devices[0]
            .volume_id
            .clone();
        state.volumes.insert(
            "vol-home".to_string(),
            Volume {
                volume_id: "vol-home".to_string(),
                size_gib: 1,
                volume_type: "gp3".to_string(),
                availability_zone: "us-east-1a".to_string(),
                state: "in-use".to_string(),
                create_time: now_iso8601(),
                encrypted: true,
                kms_key_id: None,
                iops: None,
                throughput: None,
                client_token: None,
                attachment: Some(VolumeAttachment {
                    instance_id: id.clone(),
                    device: "/dev/sdh".to_string(),
                    attach_time: now_iso8601(),
                    delete_on_termination: false,
                }),
                tags: HashMap::new(),
            },
        );
        state
            .instances
            .get_mut(&id)
            .unwrap()
            .block_devices
            .push(BlockDevice {
                device_name: "/dev/sdh".to_string(),
                volume_id: "vol-home".to_string(),
                attach_time: now_iso8601(),
                delete_on_termination: false,
            });

        terminate_instances(&state, &json!({ "InstanceId": [id.clone()] })).unwrap();
        assert!(!state.volumes.contains_key(&root));
        let home = state.volumes.get("vol-home").unwrap();
        assert_eq!(home.state, "available");
        assert!(home.attachment.is_none());
        assert!(state.instances.get(&id).unwrap().block_devices.is_empty());
    }

    #[test]
    fn modify_instance_type_only_when_stopped() {
        let state = Ec2State::default();
        let resp = run(&state, &json!({ "MinCount": 1, "MaxCount": 1 })).unwrap();
        let id = resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string();
        let change = json!({ "InstanceId": id, "InstanceType": { "Value": "t3.xlarge" } });
        let err = modify_instance_attribute(&state, &change).unwrap_err();
        assert_eq!(err.code, "IncorrectInstanceState");

        stop_instances(&state, &json!({ "InstanceId": [id.clone()] })).unwrap();
        modify_instance_attribute(&state, &change).unwrap();
        assert_eq!(state.instances.get(&id).unwrap().instance_type, "t3.xlarge");

        modify_instance_attribute(
            &state,
            &json!({ "InstanceId": id, "Attribute": "userData", "Value": "aGk=" }),
        )
        .unwrap();
        assert_eq!(
            state.instances.get(&id).unwrap().user_data.as_deref(),
            Some("aGk=")
        );

        let err = modify_instance_attribute(
            &state,
            &json!({ "InstanceId": "i-missing", "InstanceType": { "Value": "t3.large" } }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInstanceID.NotFound");
    }
}
