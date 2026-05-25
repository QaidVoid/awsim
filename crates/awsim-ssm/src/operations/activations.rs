use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmActivation, SsmManagedInstance, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn create_activation(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let description = input["Description"].as_str().unwrap_or("").to_string();
    let default_instance_name = input["DefaultInstanceName"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let iam_role = input["IamRole"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IamRole is required"))?
        .to_string();
    let registration_limit = input["RegistrationLimit"].as_i64().unwrap_or(1);

    let activation_id = Uuid::new_v4().to_string();
    let activation_code = format!("code-{}", Uuid::new_v4());
    let now = now_epoch_secs();

    let a = SsmActivation {
        activation_id: activation_id.clone(),
        activation_code: activation_code.clone(),
        description,
        default_instance_name,
        iam_role,
        registration_limit,
        registrations_count: 0,
        expiration_date: now + 30 * 86_400,
        expired: false,
        created_date: now,
    };

    state.activations.insert(activation_id.clone(), a);

    Ok(json!({
        "ActivationId": activation_id,
        "ActivationCode": activation_code,
    }))
}

pub fn delete_activation(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let activation_id = input["ActivationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ActivationId is required"))?;

    if state.activations.remove(activation_id).is_none() {
        return Err(AwsError::bad_request(
            "InvalidActivation",
            format!("Activation '{activation_id}' not found"),
        ));
    }

    Ok(json!({}))
}

pub fn describe_activations(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let items: Vec<Value> = state
        .activations
        .iter()
        .map(|e| {
            let a = e.value();
            json!({
                "ActivationId": a.activation_id,
                "Description": a.description,
                "DefaultInstanceName": a.default_instance_name,
                "IamRole": a.iam_role,
                "RegistrationLimit": a.registration_limit,
                "RegistrationsCount": a.registrations_count,
                "ExpirationDate": a.expiration_date,
                "Expired": a.expired,
                "CreatedDate": a.created_date,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "ActivationList": items }))
}

pub fn describe_instance_information(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let items: Vec<Value> = state
        .managed_instances
        .iter()
        .map(|e| {
            let m = e.value();
            json!({
                "InstanceId": m.instance_id,
                "PingStatus": m.ping_status,
                "LastPingDateTime": m.last_ping_date_time,
                "AgentVersion": m.agent_version,
                "PlatformType": m.platform_type,
                "PlatformName": m.platform_name,
                "PlatformVersion": m.platform_version,
                "IamRole": m.iam_role,
                "RegistrationDate": m.registration_date,
                "ResourceType": m.resource_type,
                "Name": m.name,
                "ComputerName": m.computer_name,
                "IPAddress": m.ip_address,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "InstanceInformationList": items }))
}

pub fn deregister_managed_instance(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "InstanceId is required"))?;

    state.managed_instances.remove(instance_id);
    Ok(json!({}))
}

pub fn update_managed_instance_role(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let instance_id = input["InstanceId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "InstanceId is required"))?;
    let iam_role = input["IamRole"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "IamRole is required"))?
        .to_string();

    let now = now_epoch_secs();
    let mut entry = state
        .managed_instances
        .entry(instance_id.to_string())
        .or_insert_with(|| SsmManagedInstance {
            instance_id: instance_id.to_string(),
            ping_status: "Online".to_string(),
            last_ping_date_time: now,
            agent_version: "3.0.0".to_string(),
            platform_type: "Linux".to_string(),
            platform_name: "AWSim".to_string(),
            platform_version: "1.0".to_string(),
            iam_role: String::new(),
            registration_date: now,
            resource_type: "ManagedInstance".to_string(),
            name: instance_id.to_string(),
            computer_name: instance_id.to_string(),
            ip_address: "127.0.0.1".to_string(),
        });
    entry.iam_role = iam_role;

    Ok(json!({}))
}

pub fn describe_instance_properties(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let items: Vec<Value> = state
        .managed_instances
        .iter()
        .map(|e| {
            let m = e.value();
            json!({
                "InstanceId": m.instance_id,
                "PingStatus": m.ping_status,
                "AgentVersion": m.agent_version,
                "PlatformType": m.platform_type,
                "PlatformName": m.platform_name,
                "IPAddress": m.ip_address,
                "ComputerName": m.computer_name,
                "ResourceType": m.resource_type,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "InstanceProperties": items }))
}
