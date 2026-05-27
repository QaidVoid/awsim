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

/// Validate broker name per AWS MQ regex: 1-50 alphanumeric + `_-`.
fn validate_broker_name(name: &str) -> Result<(), AwsError> {
    if !(1..=50).contains(&name.len())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("BrokerName `{name}` must be 1-50 chars from [a-zA-Z0-9_-]."),
        ));
    }
    Ok(())
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
    let mut obj = json!({
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
    });
    if let Some(ref v) = b.encryption_options {
        obj["EncryptionOptions"] = v.clone();
    }
    if let Some(ref v) = b.logs {
        obj["Logs"] = v.clone();
    }
    if let Some(ref v) = b.maintenance_window_start_time {
        obj["MaintenanceWindowStartTime"] = v.clone();
    }
    if let Some(ref v) = b.ldap_server_metadata {
        obj["LdapServerMetadata"] = v.clone();
    }
    if let Some(ref v) = b.configuration {
        obj["Configurations"] = json!({ "Current": v });
    }
    if let Some(ref v) = b.data_replication_mode {
        obj["DataReplicationMode"] = json!(v);
    }
    obj
}

fn user_summary(u: &BrokerUser) -> Value {
    json!({
        "Username": u.username,
        "PendingChange": u.pending_change,
    })
}

fn user_describe(u: &BrokerUser) -> Value {
    // DescribeUser must never surface the password (plaintext or
    // hashed). When an UpdateUser is in flight, return its requested
    // state under `Pending`.
    json!({
        "BrokerId": u.broker_id,
        "Username": u.username,
        "ConsoleAccess": u.console_access,
        "Groups": u.groups,
        "ReplicationUser": u.replication_user,
        "Pending": u.pending,
    })
}

pub fn create_broker(
    state: &MqState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = new_id();
    let name = require_str(input, "BrokerName")?.to_string();
    validate_broker_name(&name)?;
    if state.brokers.iter().any(|e| e.value().broker_name == name) {
        return Err(AwsError::conflict(
            "ConflictException",
            format!("Broker {name} already exists"),
        ));
    }
    let host = require_str(input, "HostInstanceType")?.to_string();
    let engine_type = require_str(input, "EngineType")?.to_string();
    let engine_version = require_str(input, "EngineVersion")?.to_string();

    // StorageType allowlist per engine. ActiveMQ accepts EFS or EBS;
    // RabbitMQ only supports EBS. Default differs per engine, so we
    // resolve the supplied (or omitted) value against the engine.
    let storage_type = input
        .get("StorageType")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| match engine_type.as_str() {
            "RABBITMQ" => "EBS".to_string(),
            _ => "EFS".to_string(),
        });
    match (engine_type.as_str(), storage_type.as_str()) {
        ("ACTIVEMQ", "EFS" | "EBS") => {}
        ("RABBITMQ", "EBS") => {}
        _ => {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!(
                    "StorageType `{storage_type}` is not supported with engine `{engine_type}`."
                ),
            ));
        }
    }

    // LDAP authentication requires server metadata; rejecting up front
    // matches what real MQ does instead of accepting a half-broken
    // broker config.
    let authentication_strategy = input
        .get("AuthenticationStrategy")
        .and_then(|v| v.as_str())
        .unwrap_or("SIMPLE")
        .to_string();
    if authentication_strategy == "LDAP"
        && input
            .get("LdapServerMetadata")
            .and_then(|v| v.as_object())
            .is_none_or(|m| m.is_empty())
    {
        return Err(AwsError::bad_request(
            "BadRequestException",
            "LdapServerMetadata is required when AuthenticationStrategy is LDAP.",
        ));
    }

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
        authentication_strategy,
        storage_type,
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
        encryption_options: input.get("EncryptionOptions").cloned(),
        logs: input.get("Logs").cloned(),
        maintenance_window_start_time: input.get("MaintenanceWindowStartTime").cloned(),
        ldap_server_metadata: input.get("LdapServerMetadata").cloned(),
        configuration: input.get("Configuration").cloned(),
        data_replication_mode: input
            .get("DataReplicationMode")
            .and_then(|v| v.as_str())
            .map(String::from),
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
                password_hash: u
                    .get("Password")
                    .and_then(|v| v.as_str())
                    .map(hash_password),
                pending: None,
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
    let password_hash = input
        .get("Password")
        .and_then(|v| v.as_str())
        .map(hash_password);
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
        password_hash,
        pending: None,
    };
    state.users.insert(key, u);
    Ok(json!({}))
}

/// SHA-256 hex digest. Lets us store + compare passwords without
/// roundtripping the plaintext through state or describe responses.
fn hash_password(password: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(password.as_bytes());
    format!("{:x}", h.finalize())
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
    // AWS persists UpdateUser as a pending change; the values stay
    // visible under `Pending` until the broker is rebooted, at which
    // point they replace the live values. Mirror that by writing the
    // requested fields into `pending` rather than the live fields.
    let console_access = input
        .get("ConsoleAccess")
        .and_then(|v| v.as_bool())
        .unwrap_or(u.console_access);
    let groups = input
        .get("Groups")
        .and_then(|v| v.as_array())
        .map(|g| {
            g.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| u.groups.clone());
    let replication_user = input
        .get("ReplicationUser")
        .and_then(|v| v.as_bool())
        .unwrap_or(u.replication_user);
    u.pending = Some(json!({
        "ConsoleAccess": console_access,
        "Groups": groups,
        "ReplicationUser": replication_user,
    }));
    // Password changes update the hash immediately but never surface
    // back to the caller.
    if let Some(p) = input.get("Password").and_then(|v| v.as_str()) {
        u.password_hash = Some(hash_password(p));
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
    let created = now();
    let description = input
        .get("Description")
        .and_then(|v| v.as_str())
        .map(String::from);
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
        created,
        latest_revision: 1,
        description: description.clone(),
        revisions: vec![crate::state::ConfigurationRevision {
            revision: 1,
            created,
            description,
            data: String::new(),
        }],
    };
    let result = json!({
        "Id": c.configuration_id,
        "Arn": c.configuration_arn,
        "Name": c.name,
        "Created": c.created,
        "LatestRevision": {
            "Revision": c.latest_revision,
            "Created": c.created,
            "Description": c.description,
        },
    });
    state.configurations.insert(id, c);
    Ok(result)
}

/// Engine-specific config-data validation. AWS rejects ActiveMQ
/// payloads that don't start with the `<broker>` root element and
/// RabbitMQ payloads that don't parse as cuttlefish-style INI. We
/// can't ship a full cuttlefish/XML parser here, so we apply a
/// lightweight signature check on the decoded bytes.
fn validate_configuration_data(engine_type: &str, decoded: &[u8]) -> Result<(), AwsError> {
    let text = std::str::from_utf8(decoded).map_err(|_| {
        AwsError::bad_request(
            "BadRequestException",
            "Configuration data must decode to UTF-8 text.",
        )
    })?;
    match engine_type {
        "ACTIVEMQ" if !text.trim_start().starts_with("<broker") => {
            return Err(AwsError::bad_request(
                "BadRequestException",
                "ActiveMQ configuration must begin with a `<broker ...>` root element.",
            ));
        }
        "RABBITMQ" => {
            // RabbitMQ cuttlefish style: lines of `key = value` (with
            // comments / blank lines allowed). Reject when every
            // non-blank line looks XML-shaped — i.e., it's an ActiveMQ
            // payload misrouted onto a RabbitMQ configuration.
            let non_blank: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            if non_blank.is_empty() {
                return Err(AwsError::bad_request(
                    "BadRequestException",
                    "RabbitMQ configuration must contain at least one `key = value` directive.",
                ));
            }
            if non_blank.iter().all(|l| l.starts_with('<')) {
                return Err(AwsError::bad_request(
                    "BadRequestException",
                    "RabbitMQ configuration must use cuttlefish (`key = value`) syntax, not XML.",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// `UpdateConfiguration`. AWS bumps `Revision`, persists the new
/// payload, and returns `LatestRevision` so callers (and CloudFormation)
/// can pin a broker to the new revision.
pub fn update_configuration(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "ConfigurationId")?;
    let data = require_str(input, "Data")?.to_string();
    let description = input
        .get("Description")
        .and_then(|v| v.as_str())
        .map(String::from);

    let engine_type = state
        .configurations
        .get(id)
        .map(|c| c.engine_type.clone())
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("Configuration {id} not found"))
        })?;
    let decoded = base64_decode(&data)?;
    validate_configuration_data(&engine_type, &decoded)?;

    let mut c = state.configurations.get_mut(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Configuration {id} not found"))
    })?;
    let new_revision = c.latest_revision + 1;
    let created = now();
    c.latest_revision = new_revision;
    c.description = description.clone();
    c.revisions.push(crate::state::ConfigurationRevision {
        revision: new_revision,
        created,
        description: description.clone(),
        data,
    });

    Ok(json!({
        "Id": c.configuration_id,
        "Arn": c.configuration_arn,
        "Name": c.name,
        "Created": c.created,
        "LatestRevision": {
            "Revision": new_revision,
            "Created": created,
            "Description": description,
        },
        // AWS returns a `Warnings` array when the engine validator
        // flagged anything non-fatal. We don't run a real validator,
        // so the field is always empty but present for shape parity.
        "Warnings": [],
    }))
}

/// `DescribeConfigurationRevision`. AWS lets callers fetch a
/// historical revision by id; we walk the in-memory history.
pub fn describe_configuration_revision(
    state: &MqState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = require_str(input, "ConfigurationId")?;
    let requested: u32 = require_str(input, "ConfigurationRevision")?
        .parse()
        .map_err(|_| {
            AwsError::bad_request(
                "BadRequestException",
                "ConfigurationRevision must be a positive integer.",
            )
        })?;
    let c = state.configurations.get(id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Configuration {id} not found"))
    })?;
    let rev = c
        .revisions
        .iter()
        .find(|r| r.revision == requested)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Configuration {id} has no revision {requested}"),
            )
        })?;
    Ok(json!({
        "ConfigurationId": c.configuration_id,
        "Created": rev.created,
        "Description": rev.description,
        "Data": rev.data,
    }))
}

fn base64_decode(s: &str) -> Result<Vec<u8>, AwsError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| {
            AwsError::bad_request(
                "BadRequestException",
                "Configuration Data must be valid base64.",
            )
        })
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
