use awsim_core::tags::{TagOpts, validate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::ids::{new_ec2_id, now_iso8601};
use crate::operations::filters::{matches_all, parse_filters, tag_field};
use crate::operations::instances::{collect_id_param, record_tags};
use crate::state::{BlockDevice, Ec2State, Volume, VolumeAttachment};

pub(crate) fn volume_to_value(v: &Volume) -> Value {
    let tags: Vec<Value> = v
        .tags
        .iter()
        .map(|(k, val)| json!({ "key": k, "value": val }))
        .collect();
    let attachments: Vec<Value> = v
        .attachment
        .iter()
        .map(|a| {
            json!({
                "volumeId": v.volume_id,
                "instanceId": a.instance_id,
                "device": a.device,
                "status": "attached",
                "attachTime": a.attach_time,
                "deleteOnTermination": a.delete_on_termination,
            })
        })
        .collect();
    let mut value = json!({
        "volumeId": v.volume_id,
        "size": v.size_gib,
        "availabilityZone": v.availability_zone,
        "status": v.state,
        "createTime": v.create_time,
        "volumeType": v.volume_type,
        "encrypted": v.encrypted,
        "multiAttachEnabled": false,
        "attachmentSet": { "item": attachments },
        "tagSet": { "item": tags },
    });
    if let Some(key) = &v.kms_key_id {
        value["kmsKeyId"] = json!(key);
    }
    if let Some(iops) = v.iops {
        value["iops"] = json!(iops);
    }
    if let Some(throughput) = v.throughput {
        value["throughput"] = json!(throughput);
    }
    value
}

fn volume_not_found(id: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidVolume.NotFound",
        format!("The volume '{id}' does not exist."),
    )
}

fn instance_not_found(id: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidInstanceID.NotFound",
        format!("The instance ID '{id}' does not exist"),
    )
}

fn int_param(input: &Value, key: &str) -> Option<i64> {
    match &input[key] {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// CreateVolume
// ---------------------------------------------------------------------------

/// A validated `CreateVolume`, not yet recorded.
pub(crate) enum CreatePlan {
    /// The client token matched an earlier request; this is its answer.
    Existing(Value),
    New(Box<Volume>),
}

pub(crate) fn plan_create_volume(
    state: &Ec2State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<CreatePlan, AwsError> {
    let client_token = input["ClientToken"].as_str().map(str::to_string);
    if let Some(token) = &client_token
        && let Some(existing) = state
            .volumes
            .iter()
            .find(|v| v.client_token.as_deref() == Some(token))
    {
        return Ok(CreatePlan::Existing(volume_to_value(existing.value())));
    }

    let availability_zone = input["AvailabilityZone"]
        .as_str()
        .ok_or_else(|| crate::error::missing_parameter("AvailabilityZone"))?;
    if !availability_zone.starts_with(&ctx.region) {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Invalid availability zone: [{availability_zone}]"),
        ));
    }
    let size = int_param(input, "Size").ok_or_else(|| {
        AwsError::bad_request(
            "MissingParameter",
            "The request must contain the parameter size or snapshotId",
        )
    })?;
    let volume_type = input["VolumeType"].as_str().unwrap_or("gp2").to_string();
    let max = match volume_type.as_str() {
        "gp2" | "gp3" | "io1" | "io2" => 16_384,
        "st1" | "sc1" => 16_384,
        "standard" => 1_024,
        other => {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!("Value ({other}) for parameter volumeType is invalid."),
            ));
        }
    };
    if size < 1 || size > max {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Volume of {size}GiB is outside the allowed range of 1 to {max}GiB."),
        ));
    }

    let mut tags = Vec::new();
    for spec in input["TagSpecification"].as_array().into_iter().flatten() {
        if spec["ResourceType"].as_str() != Some("volume") {
            continue;
        }
        for t in spec["Tag"].as_array().into_iter().flatten() {
            if let Some(k) = t["Key"].as_str() {
                tags.push((
                    k.to_string(),
                    t["Value"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }
    }
    validate(&tags, &TagOpts::aws_default())?;

    let kms_key_id = input["KmsKeyId"].as_str().map(str::to_string);
    let encrypted = input["Encrypted"].as_bool() == Some(true)
        || input["Encrypted"].as_str() == Some("true")
        || kms_key_id.is_some();

    Ok(CreatePlan::New(Box::new(Volume {
        volume_id: new_ec2_id("vol"),
        size_gib: size as u64,
        volume_type,
        availability_zone: availability_zone.to_string(),
        state: "available".to_string(),
        create_time: now_iso8601(),
        encrypted,
        kms_key_id,
        iops: int_param(input, "Iops"),
        throughput: int_param(input, "Throughput"),
        client_token,
        attachment: None,
        tags: tags.into_iter().collect(),
    })))
}

/// Record a planned volume and answer as `CreateVolume` does. EC2 reports
/// `creating` here and `available` from the next describe.
pub(crate) fn commit_create_volume(state: &Ec2State, volume: Volume) -> Value {
    let tags: Vec<(String, String)> = volume
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    record_tags(state, &volume.volume_id, &tags);
    let mut value = volume_to_value(&volume);
    value["status"] = json!("creating");
    info!(volume_id = %volume.volume_id, size = volume.size_gib, "CreateVolume");
    state.volumes.insert(volume.volume_id.clone(), volume);
    value
}

pub fn create_volume(
    state: &Ec2State,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    match plan_create_volume(state, input, ctx)? {
        CreatePlan::Existing(value) => Ok(value),
        CreatePlan::New(volume) => Ok(commit_create_volume(state, *volume)),
    }
}

// ---------------------------------------------------------------------------
// DescribeVolumes
// ---------------------------------------------------------------------------

pub fn describe_volumes(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let ids = collect_id_param(input, "VolumeId");
    if let Some(missing) = ids.iter().find(|id| !state.volumes.contains_key(*id)) {
        return Err(volume_not_found(missing));
    }
    let filters = parse_filters(input);
    let mut items: Vec<(String, Value)> = state
        .volumes
        .iter()
        .filter(|v| ids.is_empty() || ids.contains(&v.volume_id))
        .filter(|v| {
            matches_all(&filters, |name| match name {
                "volume-id" => Some(vec![v.volume_id.clone()]),
                "status" => Some(vec![v.state.clone()]),
                "availability-zone" => Some(vec![v.availability_zone.clone()]),
                "volume-type" => Some(vec![v.volume_type.clone()]),
                "attachment.instance-id" => {
                    Some(v.attachment.iter().map(|a| a.instance_id.clone()).collect())
                }
                "attachment.device" => {
                    Some(v.attachment.iter().map(|a| a.device.clone()).collect())
                }
                _ => tag_field(&v.tags, name),
            })
        })
        .map(|v| (v.volume_id.clone(), volume_to_value(v.value())))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let items: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();
    Ok(json!({ "volumeSet": { "item": items } }))
}

// ---------------------------------------------------------------------------
// AttachVolume
// ---------------------------------------------------------------------------

/// A validated `AttachVolume`, not yet recorded.
pub(crate) struct AttachPlan {
    pub volume_id: String,
    pub instance_id: String,
    pub device: String,
}

pub(crate) fn plan_attach_volume(state: &Ec2State, input: &Value) -> Result<AttachPlan, AwsError> {
    let volume_id = crate::operations::require_str(input, "VolumeId")?;
    let instance_id = crate::operations::require_str(input, "InstanceId")?;
    let device = crate::operations::require_str(input, "Device")?;

    let volume = state
        .volumes
        .get(volume_id)
        .ok_or_else(|| volume_not_found(volume_id))?;
    let instance = state
        .instances
        .get(instance_id)
        .ok_or_else(|| instance_not_found(instance_id))?;

    if !matches!(instance.state.as_str(), "running" | "stopped") {
        return Err(AwsError::bad_request(
            "IncorrectState",
            format!("Instance '{instance_id}' is not 'running'."),
        ));
    }
    if let Some(attachment) = &volume.attachment {
        return Err(AwsError::bad_request(
            "VolumeInUse",
            format!(
                "{volume_id} is already attached to an instance ({})",
                attachment.instance_id
            ),
        ));
    }
    if volume.availability_zone != instance.availability_zone {
        return Err(AwsError::bad_request(
            "InvalidVolume.ZoneMismatch",
            format!(
                "The volume '{volume_id}' is not in the same availability zone as instance '{instance_id}'"
            ),
        ));
    }
    if instance
        .block_devices
        .iter()
        .any(|d| d.device_name == device)
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Attachment point {device} is already in use"),
        ));
    }
    Ok(AttachPlan {
        volume_id: volume_id.to_string(),
        instance_id: instance_id.to_string(),
        device: device.to_string(),
    })
}

/// Record a planned attachment and answer as `AttachVolume` does.
pub(crate) fn commit_attach_volume(state: &Ec2State, plan: &AttachPlan) -> Value {
    let now = now_iso8601();
    if let Some(mut volume) = state.volumes.get_mut(&plan.volume_id) {
        volume.state = "in-use".to_string();
        volume.attachment = Some(VolumeAttachment {
            instance_id: plan.instance_id.clone(),
            device: plan.device.clone(),
            attach_time: now.clone(),
            delete_on_termination: false,
        });
    }
    if let Some(mut instance) = state.instances.get_mut(&plan.instance_id) {
        instance.block_devices.push(BlockDevice {
            device_name: plan.device.clone(),
            volume_id: plan.volume_id.clone(),
            attach_time: now.clone(),
            delete_on_termination: false,
        });
    }
    info!(volume_id = %plan.volume_id, instance_id = %plan.instance_id, device = %plan.device, "AttachVolume");
    json!({
        "volumeId": plan.volume_id,
        "instanceId": plan.instance_id,
        "device": plan.device,
        "status": "attaching",
        "attachTime": now,
    })
}

pub fn attach_volume(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let plan = plan_attach_volume(state, input)?;
    Ok(commit_attach_volume(state, &plan))
}

// ---------------------------------------------------------------------------
// DetachVolume
// ---------------------------------------------------------------------------

/// A validated `DetachVolume`, not yet recorded.
pub(crate) struct DetachPlan {
    pub volume_id: String,
    pub instance_id: String,
    pub device: String,
    pub attach_time: String,
}

pub(crate) fn plan_detach_volume(state: &Ec2State, input: &Value) -> Result<DetachPlan, AwsError> {
    let volume_id = crate::operations::require_str(input, "VolumeId")?;
    let volume = state
        .volumes
        .get(volume_id)
        .ok_or_else(|| volume_not_found(volume_id))?;
    let Some(attachment) = volume.attachment.clone() else {
        return Err(AwsError::bad_request(
            "IncorrectState",
            format!("Volume '{volume_id}' is in the '{}' state.", volume.state),
        ));
    };
    if let Some(asked) = input["InstanceId"].as_str()
        && asked != attachment.instance_id
    {
        return Err(AwsError::bad_request(
            "InvalidAttachment.NotFound",
            format!("Volume '{volume_id}' is not attached to instance '{asked}'."),
        ));
    }
    if let Some(asked) = input["Device"].as_str()
        && asked != attachment.device
    {
        return Err(AwsError::bad_request(
            "InvalidAttachment.NotFound",
            format!("Volume '{volume_id}' is not attached at '{asked}'."),
        ));
    }
    let instance = state.instances.get(&attachment.instance_id);
    let running = instance.as_ref().is_some_and(|i| i.state == "running");
    let is_root = instance
        .as_ref()
        .is_some_and(|i| i.root_device_name == attachment.device);
    if is_root && running {
        return Err(AwsError::bad_request(
            "OperationNotPermitted",
            format!(
                "Unable to detach root volume '{volume_id}' from instance '{}'",
                attachment.instance_id
            ),
        ));
    }
    Ok(DetachPlan {
        volume_id: volume_id.to_string(),
        instance_id: attachment.instance_id,
        device: attachment.device,
        attach_time: attachment.attach_time,
    })
}

/// Record a planned detachment and answer as `DetachVolume` does.
pub(crate) fn commit_detach_volume(state: &Ec2State, plan: &DetachPlan) -> Value {
    if let Some(mut volume) = state.volumes.get_mut(&plan.volume_id) {
        volume.state = "available".to_string();
        volume.attachment = None;
    }
    if let Some(mut instance) = state.instances.get_mut(&plan.instance_id) {
        instance
            .block_devices
            .retain(|d| d.volume_id != plan.volume_id);
    }
    info!(volume_id = %plan.volume_id, instance_id = %plan.instance_id, "DetachVolume");
    json!({
        "volumeId": plan.volume_id,
        "instanceId": plan.instance_id,
        "device": plan.device,
        "status": "detaching",
        "attachTime": plan.attach_time,
    })
}

pub fn detach_volume(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let plan = plan_detach_volume(state, input)?;
    Ok(commit_detach_volume(state, &plan))
}

// ---------------------------------------------------------------------------
// DeleteVolume
// ---------------------------------------------------------------------------

/// Check a `DeleteVolume` and answer the id of the volume to delete.
pub(crate) fn plan_delete_volume(state: &Ec2State, input: &Value) -> Result<String, AwsError> {
    let volume_id = crate::operations::require_str(input, "VolumeId")?;
    let volume = state
        .volumes
        .get(volume_id)
        .ok_or_else(|| volume_not_found(volume_id))?;
    if let Some(attachment) = &volume.attachment {
        return Err(AwsError::bad_request(
            "VolumeInUse",
            format!(
                "Volume {volume_id} is currently attached to {}",
                attachment.instance_id
            ),
        ));
    }
    Ok(volume_id.to_string())
}

pub(crate) fn commit_delete_volume(state: &Ec2State, volume_id: &str) -> Value {
    state.volumes.remove(volume_id);
    state.resource_tags.remove(volume_id);
    info!(volume_id, "DeleteVolume");
    json!({ "return": true })
}

pub fn delete_volume(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let volume_id = plan_delete_volume(state, input)?;
    Ok(commit_delete_volume(state, &volume_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::instances::{run_instances, stop_instances, terminate_instances};

    fn ctx() -> RequestContext {
        RequestContext::new("ec2", "us-east-1")
    }

    fn launch(state: &Ec2State) -> String {
        let resp = run_instances(state, &json!({ "MinCount": 1, "MaxCount": 1 }), &ctx()).unwrap();
        resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn create(state: &Ec2State, extra: Value) -> String {
        let mut input = json!({
            "AvailabilityZone": "us-east-1a",
            "Size": "1",
            "VolumeType": "gp3",
            "Encrypted": "true",
            "KmsKeyId": "alias/ws",
            "TagSpecification": [{ "ResourceType": "volume", "Tag": [{ "Key": "team", "Value": "a" }] }],
        });
        for (k, v) in extra.as_object().unwrap() {
            input[k] = v.clone();
        }
        let resp = create_volume(state, &input, &ctx()).unwrap();
        assert_eq!(resp["status"], "creating");
        resp["volumeId"].as_str().unwrap().to_string()
    }

    fn describe_one(state: &Ec2State, id: &str) -> Value {
        describe_volumes(state, &json!({ "VolumeId": [id] })).unwrap()["volumeSet"]["item"][0]
            .clone()
    }

    #[test]
    fn volume_lifecycle_round_trip() {
        let state = Ec2State::default();
        let instance = launch(&state);
        let volume = create(&state, json!({}));
        let described = describe_one(&state, &volume);
        assert_eq!(described["status"], "available");
        assert_eq!(described["encrypted"], true);
        assert_eq!(described["tagSet"]["item"][0]["key"], "team");

        let attached = attach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": instance, "Device": "/dev/sdh" }),
        )
        .unwrap();
        assert_eq!(attached["status"], "attaching");
        let described = describe_one(&state, &volume);
        assert_eq!(described["status"], "in-use");
        assert_eq!(
            described["attachmentSet"]["item"][0]["instanceId"],
            json!(instance)
        );
        let devices = state
            .instances
            .get(&instance)
            .unwrap()
            .block_devices
            .clone();
        assert!(devices.iter().any(|d| d.device_name == "/dev/sdh"));

        let err = delete_volume(&state, &json!({ "VolumeId": volume })).unwrap_err();
        assert_eq!(err.code, "VolumeInUse");

        detach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": instance }),
        )
        .unwrap();
        assert_eq!(describe_one(&state, &volume)["status"], "available");
        delete_volume(&state, &json!({ "VolumeId": volume })).unwrap();
        let err = describe_volumes(&state, &json!({ "VolumeId": [volume] })).unwrap_err();
        assert_eq!(err.code, "InvalidVolume.NotFound");
    }

    #[test]
    fn create_volume_honours_client_token() {
        let state = Ec2State::default();
        let first = create(&state, json!({ "ClientToken": "t" }));
        let again = create_volume(
            &state,
            &json!({ "AvailabilityZone": "us-east-1a", "Size": "1", "ClientToken": "t" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(again["volumeId"], json!(first));
        assert_eq!(state.volumes.len(), 1);
    }

    #[test]
    fn create_volume_validates_input() {
        let state = Ec2State::default();
        let err = create_volume(&state, &json!({ "Size": "1" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "MissingParameter");
        let err = create_volume(&state, &json!({ "AvailabilityZone": "us-east-1a" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "MissingParameter");
        let err = create_volume(
            &state,
            &json!({ "AvailabilityZone": "us-east-1a", "Size": "0" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn attach_refuses_busy_device_attached_volume_and_bad_state() {
        let state = Ec2State::default();
        let instance = launch(&state);
        let a = create(&state, json!({}));
        let b = create(&state, json!({}));
        attach_volume(
            &state,
            &json!({ "VolumeId": a, "InstanceId": instance, "Device": "/dev/sdh" }),
        )
        .unwrap();
        let err = attach_volume(
            &state,
            &json!({ "VolumeId": a, "InstanceId": instance, "Device": "/dev/sdi" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "VolumeInUse");
        let err = attach_volume(
            &state,
            &json!({ "VolumeId": b, "InstanceId": instance, "Device": "/dev/sdh" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");

        state.instances.get_mut(&instance).unwrap().state = "pending".to_string();
        let err = attach_volume(
            &state,
            &json!({ "VolumeId": b, "InstanceId": instance, "Device": "/dev/sdi" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "IncorrectState");

        let err = attach_volume(
            &state,
            &json!({ "VolumeId": b, "InstanceId": "i-nope", "Device": "/dev/sdi" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInstanceID.NotFound");
    }

    #[test]
    fn attach_refuses_other_zone() {
        let state = Ec2State::default();
        let instance = launch(&state);
        let volume = create(&state, json!({ "AvailabilityZone": "us-east-1b" }));
        let err = attach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": instance, "Device": "/dev/sdh" }),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidVolume.ZoneMismatch");
    }

    #[test]
    fn detach_refuses_root_of_running_instance_and_unattached_volume() {
        let state = Ec2State::default();
        let instance = launch(&state);
        let root = state.instances.get(&instance).unwrap().block_devices[0]
            .volume_id
            .clone();
        let err = detach_volume(&state, &json!({ "VolumeId": root })).unwrap_err();
        assert_eq!(err.code, "OperationNotPermitted");
        stop_instances(&state, &json!({ "InstanceId": [instance] })).unwrap();
        detach_volume(&state, &json!({ "VolumeId": root })).unwrap();

        let err = detach_volume(&state, &json!({ "VolumeId": root })).unwrap_err();
        assert_eq!(err.code, "IncorrectState");
    }

    #[test]
    fn describe_volumes_filters_by_tag_and_attachment() {
        let state = Ec2State::default();
        let instance = launch(&state);
        let volume = create(&state, json!({}));
        attach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": instance, "Device": "/dev/sdh" }),
        )
        .unwrap();
        let by_tag = describe_volumes(
            &state,
            &json!({ "Filter": [{ "Name": "tag:team", "Value": ["a"] }] }),
        )
        .unwrap();
        assert_eq!(by_tag["volumeSet"]["item"].as_array().unwrap().len(), 1);
        let by_instance = describe_volumes(
            &state,
            &json!({ "Filter": [{ "Name": "attachment.instance-id", "Value": [instance] }] }),
        )
        .unwrap();
        // The root volume and the attached one.
        assert_eq!(
            by_instance["volumeSet"]["item"].as_array().unwrap().len(),
            2
        );

        terminate_instances(&state, &json!({ "InstanceId": [instance] })).unwrap();
        assert_eq!(describe_one(&state, &volume)["status"], "available");
    }
}
