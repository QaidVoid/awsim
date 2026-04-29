use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Broker, BrokerUser, Configuration, MqState, user_key};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

fn new_id() -> String {
    format!("b-{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

fn broker_arn(ctx: &RequestContext, id: &str) -> String {
    format!("arn:aws:mq:{}:{}:broker:{}", ctx.region, ctx.account_id, id)
}

fn config_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:mq:{}:{}:configuration:{}",
        ctx.region, ctx.account_id, id
    )
}

fn broker_summary(b: &Broker) -> Value {
    json!({
        "BrokerId": b.broker_id,
        "BrokerArn": b.broker_arn,
        "BrokerName": b.broker_name,
        "BrokerState": b.broker_state,
        "DeploymentMode": b.deployment_mode,
        "EngineType": b.engine_type,
        "HostInstanceType": b.host_instance_type,
        "Created": b.created,
    })
}

fn broker_describe(b: &Broker, users: Vec<Value>) -> Value {
    json!({
        "BrokerId": b.broker_id,
        "BrokerArn": b.broker_arn,
        "BrokerName": b.broker_name,
        "BrokerState": b.broker_state,
        "BrokerInstances": [{
            "Endpoints": [format!("ssl://{}.mq.{}.amazonaws.com:61617", b.broker_id, "us-east-1")],
            "ConsoleURL": format!("https://{}.mq.us-east-1.amazonaws.com:8162", b.broker_id),
            "IpAddress": "10.0.0.10",
        }],
        "AutoMinorVersionUpgrade": b.auto_minor_version_upgrade,
        "DeploymentMode": b.deployment_mode,
        "EngineType": b.engine_type,
        "EngineVersion": b.engine_version,
        "HostInstanceType": b.host_instance_type,
        "PubliclyAccessible": b.publicly_accessible,
        "Created": b.created,
        "AuthenticationStrategy": b.authentication_strategy,
        "StorageType": b.storage_type,
        "SecurityGroups": b.security_groups,
        "SubnetIds": b.subnet_ids,
        "Tags": b.tags,
        "Users": users,
    })
}

fn user_summary(u: &BrokerUser) -> Value {
    json!({
        "Username": u.username,
        "PendingChange": u.pending_change,
    })
}

fn user_describe(u: &BrokerUser) -> Value {
    json!({
        "BrokerId": u.broker_id,
        "Username": u.username,
        "ConsoleAccess": u.console_access,
        "Groups": u.groups,
        "ReplicationUser": u.replication_user,
        "Pending": null,
    })
}

pub fn create_broker(
    state: &MqState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = new_id();
    let name = require_str(input, "BrokerName")?.to_string();
    if state.brokers.iter().any(|e| e.value().broker_name == name) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("Broker {name} already exists"),
        ));
    }
    let host = require_str(input, "HostInstanceType")?.to_string();
    let engine_type = require_str(input, "EngineType")?.to_string();
    let engine_version = require_str(input, "EngineVersion")?.to_string();

    let tags: HashMap<String, String> = input
        .get("Tags")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let b = Broker {
        broker_id: id.clone(),
        broker_arn: broker_arn(ctx, &id),
        broker_name: name,
        broker_state: "RUNNING".to_string(),
        broker_instance_type: host.clone(),
        deployment_mode: input
            .get("DeploymentMode")
            .and_then(|v| v.as_str())
            .unwrap_or("SINGLE_INSTANCE")
            .to_string(),
        engine_type,
        engine_version,
        auto_minor_version_upgrade: input
            .get("AutoMinorVersionUpgrade")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        publicly_accessible: input
            .get("PubliclyAccessible")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        host_instance_type: host,
        created: now(),
        authentication_strategy: input
            .get("AuthenticationStrategy")
            .and_then(|v| v.as_str())
            .unwrap_or("SIMPLE")
            .to_string(),
        storage_type: input
            .get("StorageType")
            .and_then(|v| v.as_str())
            .unwrap_or("EFS")
            .to_string(),
        security_groups: input
            .get("SecurityGroups")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        subnet_ids: input
            .get("SubnetIds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        tags,
    };
    let result = json!({ "BrokerId": id, "BrokerArn": b.broker_arn });
    state.brokers.insert(id.clone(), b);

    // Initial users from CreateBroker.Users[]
    if let Some(users) = input.get("Users").and_then(|v| v.as_array()) {
        for u in users {
            let username = match u.get("Username").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let user = BrokerUser {
                broker_id: id.clone(),
                username: username.clone(),
                console_access: u
                    .get("ConsoleAccess")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                groups: u
                    .get("Groups")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                replication_user: u
                    .get("ReplicationUser")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                pending_change: None,
            };
            state.users.insert(user_key(&id, &username), user);
        }
    }
    Ok(result)
}

pub fn describe_broker(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "BrokerId")?;
    let b = state.brokers.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Broker {id} not found"))
    })?;
    let users: Vec<Value> = state
        .users
        .iter()
        .filter(|e| e.value().broker_id == id)
        .map(|e| user_summary(e.value()))
        .collect();
    Ok(broker_describe(&b, users))
}

pub fn list_brokers(
    state: &MqState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .brokers
        .iter()
        .map(|e| broker_summary(e.value()))
        .collect();
    Ok(json!({ "BrokerSummaries": items }))
}

pub fn delete_broker(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "BrokerId")?;
    state.brokers.remove(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Broker {id} not found"))
    })?;
    let prefix = format!("{id}|");
    state.users.retain(|k, _| !k.starts_with(&prefix));
    Ok(json!({ "BrokerId": id }))
}

pub fn update_broker(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "BrokerId")?;
    let mut b = state.brokers.get_mut(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Broker {id} not found"))
    })?;
    if let Some(host) = input.get("HostInstanceType").and_then(|v| v.as_str()) {
        b.host_instance_type = host.to_string();
        b.broker_instance_type = host.to_string();
    }
    if let Some(v) = input.get("EngineVersion").and_then(|v| v.as_str()) {
        b.engine_version = v.to_string();
    }
    if let Some(b2) = input
        .get("AutoMinorVersionUpgrade")
        .and_then(|v| v.as_bool())
    {
        b.auto_minor_version_upgrade = b2;
    }
    Ok(json!({
        "BrokerId": b.broker_id,
        "AutoMinorVersionUpgrade": b.auto_minor_version_upgrade,
        "EngineVersion": b.engine_version,
        "HostInstanceType": b.host_instance_type,
    }))
}

pub fn reboot_broker(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "BrokerId")?;
    let mut b = state.brokers.get_mut(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Broker {id} not found"))
    })?;
    b.broker_state = "RUNNING".to_string();
    Ok(json!({}))
}

pub fn create_user(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let broker_id = require_str(input, "BrokerId")?.to_string();
    let username = require_str(input, "Username")?.to_string();
    if !state.brokers.contains_key(&broker_id) {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Broker {broker_id} not found"),
        ));
    }
    let key = user_key(&broker_id, &username);
    if state.users.contains_key(&key) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("User {username} already exists"),
        ));
    }
    let u = BrokerUser {
        broker_id,
        username,
        console_access: input
            .get("ConsoleAccess")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        groups: input
            .get("Groups")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        replication_user: input
            .get("ReplicationUser")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        pending_change: Some("CREATE".to_string()),
    };
    state.users.insert(key, u);
    Ok(json!({}))
}

pub fn describe_user(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let broker_id = require_str(input, "BrokerId")?;
    let username = require_str(input, "Username")?;
    let u = state
        .users
        .get(&user_key(broker_id, username))
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("User {username} not found"))
        })?;
    Ok(user_describe(&u))
}

pub fn list_users(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let broker_id = require_str(input, "BrokerId")?;
    let items: Vec<Value> = state
        .users
        .iter()
        .filter(|e| e.value().broker_id == broker_id)
        .map(|e| user_summary(e.value()))
        .collect();
    Ok(json!({ "BrokerId": broker_id, "Users": items }))
}

pub fn delete_user(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let broker_id = require_str(input, "BrokerId")?;
    let username = require_str(input, "Username")?;
    state
        .users
        .remove(&user_key(broker_id, username))
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("User {username} not found"))
        })?;
    Ok(json!({}))
}

pub fn update_user(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let broker_id = require_str(input, "BrokerId")?;
    let username = require_str(input, "Username")?;
    let mut u = state
        .users
        .get_mut(&user_key(broker_id, username))
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("User {username} not found"))
        })?;
    if let Some(c) = input.get("ConsoleAccess").and_then(|v| v.as_bool()) {
        u.console_access = c;
    }
    if let Some(g) = input.get("Groups").and_then(|v| v.as_array()) {
        u.groups = g
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    u.pending_change = Some("UPDATE".to_string());
    Ok(json!({}))
}

pub fn create_configuration(
    state: &MqState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = uuid::Uuid::new_v4().to_string();
    let c = Configuration {
        configuration_id: id.clone(),
        configuration_arn: config_arn(ctx, &id),
        name: require_str(input, "Name")?.to_string(),
        engine_type: require_str(input, "EngineType")?.to_string(),
        engine_version: require_str(input, "EngineVersion")?.to_string(),
        authentication_strategy: input
            .get("AuthenticationStrategy")
            .and_then(|v| v.as_str())
            .unwrap_or("SIMPLE")
            .to_string(),
        created: now(),
        latest_revision: 1,
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let result = json!({
        "Id": c.configuration_id,
        "Arn": c.configuration_arn,
        "Name": c.name,
        "Created": c.created,
    });
    state.configurations.insert(id, c);
    Ok(result)
}

pub fn describe_configuration(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "ConfigurationId")?;
    let c = state.configurations.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Configuration {id} not found"))
    })?;
    Ok(json!({
        "Id": c.configuration_id,
        "Arn": c.configuration_arn,
        "Name": c.name,
        "EngineType": c.engine_type,
        "EngineVersion": c.engine_version,
        "AuthenticationStrategy": c.authentication_strategy,
        "Description": c.description,
        "Created": c.created,
        "LatestRevision": { "Revision": c.latest_revision, "Created": c.created, "Description": c.description },
    }))
}

pub fn list_configurations(
    state: &MqState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .configurations
        .iter()
        .map(|e| {
            let c = e.value();
            json!({
                "Id": c.configuration_id,
                "Arn": c.configuration_arn,
                "Name": c.name,
                "EngineType": c.engine_type,
                "EngineVersion": c.engine_version,
                "Created": c.created,
            })
        })
        .collect();
    Ok(json!({ "Configurations": items }))
}
