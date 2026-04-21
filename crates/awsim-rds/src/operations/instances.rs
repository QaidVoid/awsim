use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        db_instance_already_exists, db_instance_not_found, invalid_db_instance_state,
        invalid_parameter,
    },
    ids::{
        default_engine_version, default_port, instance_arn, instance_endpoint, now_iso8601,
    },
    state::{DbEndpoint, DbInstance, RdsState},
};

use super::{opt_bool, opt_str, opt_u32, require_str};

fn instance_to_value(inst: &DbInstance) -> Value {
    let endpoint = inst.endpoint.as_ref().map(|e| {
        json!({
            "Address": e.address,
            "Port": e.port,
        })
    });

    json!({
        "DBInstanceIdentifier": inst.identifier,
        "DBInstanceArn": inst.arn,
        "DBInstanceClass": inst.instance_class,
        "Engine": inst.engine,
        "EngineVersion": inst.engine_version,
        "DBInstanceStatus": inst.status,
        "MasterUsername": inst.master_username,
        "AllocatedStorage": inst.allocated_storage,
        "Endpoint": endpoint,
        "DBSubnetGroup": inst.subnet_group_name.as_deref().map(|n| json!({ "DBSubnetGroupName": n })),
        "VpcSecurityGroups": inst.vpc_security_groups.iter().map(|sg| json!({ "VpcSecurityGroupId": sg, "Status": "active" })).collect::<Vec<_>>(),
        "MultiAZ": inst.multi_az,
        "PubliclyAccessible": inst.publicly_accessible,
        "StorageType": inst.storage_type,
        "DBClusterIdentifier": inst.cluster_identifier,
        "InstanceCreateTime": inst.created_at,
    })
}

pub fn create_db_instance(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;
    let instance_class = require_str(input, "DBInstanceClass")?;
    let engine = require_str(input, "Engine")?;
    let master_username = require_str(input, "MasterUsername")?;
    // MasterUserPassword is required for real AWS but we just store it (never expose it).
    let _master_password = require_str(input, "MasterUserPassword")?;
    let allocated_storage = opt_u32(input, "AllocatedStorage").unwrap_or(20);
    let subnet_group_name = opt_str(input, "DBSubnetGroupName").map(|s| s.to_string());
    let multi_az = opt_bool(input, "MultiAZ").unwrap_or(false);
    let publicly_accessible = opt_bool(input, "PubliclyAccessible").unwrap_or(false);
    let storage_type = opt_str(input, "StorageType").unwrap_or("gp2").to_string();

    // Validate engine
    match engine {
        "postgres" | "mysql" | "mariadb" | "oracle-ee" | "sqlserver-ex" | "sqlserver-se"
        | "sqlserver-ee" | "sqlserver-web" => {}
        _ => {
            return Err(invalid_parameter(format!("Unknown engine: {engine}")));
        }
    }

    if state.instances.contains_key(identifier) {
        return Err(db_instance_already_exists(identifier));
    }

    let engine_version = opt_str(input, "EngineVersion")
        .unwrap_or_else(|| default_engine_version(engine))
        .to_string();

    let arn = instance_arn(&ctx.region, &ctx.account_id, identifier);
    let address = instance_endpoint(identifier, &ctx.region);
    let port = default_port(engine);

    let inst = DbInstance {
        identifier: identifier.to_string(),
        arn: arn.clone(),
        instance_class: instance_class.to_string(),
        engine: engine.to_string(),
        engine_version,
        status: "available".to_string(),
        master_username: master_username.to_string(),
        allocated_storage,
        endpoint: Some(DbEndpoint { address, port }),
        subnet_group_name,
        vpc_security_groups: vec![],
        multi_az,
        publicly_accessible,
        storage_type,
        cluster_identifier: opt_str(input, "DBClusterIdentifier").map(|s| s.to_string()),
        created_at: now_iso8601(),
    };

    let result = instance_to_value(&inst);
    state.instances.insert(identifier.to_string(), inst);

    Ok(json!({ "DBInstance": result }))
}

pub fn delete_db_instance(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;

    let inst = state
        .instances
        .get(identifier)
        .ok_or_else(|| db_instance_not_found(identifier))?
        .clone();

    let result = instance_to_value(&inst);
    drop(inst);
    state.instances.remove(identifier);

    Ok(json!({ "DBInstance": result }))
}

pub fn describe_db_instances(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_id = opt_str(input, "DBInstanceIdentifier");

    if let Some(id) = filter_id {
        let inst = state
            .instances
            .get(id)
            .ok_or_else(|| db_instance_not_found(id))?;
        let items = vec![instance_to_value(&inst)];
        return Ok(json!({
            "DBInstances": { "DBInstance": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .instances
        .iter()
        .map(|e| instance_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBInstances": { "DBInstance": items },
        "Marker": null,
    }))
}

pub fn modify_db_instance(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;

    let mut inst = state
        .instances
        .get_mut(identifier)
        .ok_or_else(|| db_instance_not_found(identifier))?;

    if let Some(class) = opt_str(input, "DBInstanceClass") {
        inst.instance_class = class.to_string();
    }
    if let Some(storage) = opt_u32(input, "AllocatedStorage") {
        inst.allocated_storage = storage;
    }
    if let Some(multi_az) = opt_bool(input, "MultiAZ") {
        inst.multi_az = multi_az;
    }
    if let Some(publicly_accessible) = opt_bool(input, "PubliclyAccessible") {
        inst.publicly_accessible = publicly_accessible;
    }
    if let Some(storage_type) = opt_str(input, "StorageType") {
        inst.storage_type = storage_type.to_string();
    }

    let result = instance_to_value(&inst);
    Ok(json!({ "DBInstance": result }))
}

pub fn start_db_instance(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;

    let mut inst = state
        .instances
        .get_mut(identifier)
        .ok_or_else(|| db_instance_not_found(identifier))?;

    if inst.status != "stopped" {
        return Err(invalid_db_instance_state(identifier, &inst.status));
    }

    inst.status = "available".to_string();
    let result = instance_to_value(&inst);
    Ok(json!({ "DBInstance": result }))
}

pub fn stop_db_instance(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;

    let mut inst = state
        .instances
        .get_mut(identifier)
        .ok_or_else(|| db_instance_not_found(identifier))?;

    if inst.status != "available" {
        return Err(invalid_db_instance_state(identifier, &inst.status));
    }

    inst.status = "stopped".to_string();
    let result = instance_to_value(&inst);
    Ok(json!({ "DBInstance": result }))
}

pub fn reboot_db_instance(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;

    let mut inst = state
        .instances
        .get_mut(identifier)
        .ok_or_else(|| db_instance_not_found(identifier))?;

    // Rebooting transitions back to available immediately (metadata only).
    inst.status = "available".to_string();
    let result = instance_to_value(&inst);
    Ok(json!({ "DBInstance": result }))
}
