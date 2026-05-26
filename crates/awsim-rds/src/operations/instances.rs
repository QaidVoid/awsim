use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        db_instance_already_exists, db_instance_not_found, invalid_db_instance_state,
        invalid_parameter,
    },
    ids::{default_engine_version, default_port, instance_arn, instance_endpoint, now_iso8601},
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

    let mut obj = json!({
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
    });
    if let Some(iops) = inst.iops {
        obj["Iops"] = json!(iops);
    }
    if let Some(t) = inst.storage_throughput {
        obj["StorageThroughput"] = json!(t);
    }
    obj
}

pub fn create_db_instance(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;
    validate_db_identifier(identifier)?;
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
    if !matches!(
        storage_type.as_str(),
        "gp2" | "gp3" | "io1" | "io2" | "standard"
    ) {
        return Err(invalid_parameter(format!(
            "StorageType '{storage_type}' must be one of: gp2, gp3, io1, io2, standard."
        )));
    }

    // AWS only accepts Iops on io1/io2/gp3; everything else must be
    // either omitted or zero. StorageThroughput is gp3-only.
    let iops = opt_u32(input, "Iops");
    if let Some(v) = iops
        && v > 0
        && !matches!(storage_type.as_str(), "io1" | "io2" | "gp3")
    {
        return Err(invalid_parameter(format!(
            "Iops is only supported with io1, io2, or gp3 storage; got `{storage_type}`."
        )));
    }
    let storage_throughput = opt_u32(input, "StorageThroughput");
    if let Some(v) = storage_throughput
        && v > 0
        && storage_type != "gp3"
    {
        return Err(invalid_parameter(format!(
            "StorageThroughput is only supported with gp3 storage; got `{storage_type}`."
        )));
    }

    // Validate engine
    match engine {
        "postgres" | "mysql" | "mariadb" | "oracle-ee" | "sqlserver-ex" | "sqlserver-se"
        | "sqlserver-ee" | "sqlserver-web" | "docdb" | "neptune" => {}
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

    let vpc_security_groups: Vec<String> = input["VpcSecurityGroupIds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

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
        vpc_security_groups,
        multi_az,
        publicly_accessible,
        storage_type,
        cluster_identifier: opt_str(input, "DBClusterIdentifier").map(|s| s.to_string()),
        created_at: now_iso8601(),
        iops,
        storage_throughput,
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

/// Validate an RDS DBInstanceIdentifier against AWS's constraint:
/// 1-63 characters, must start with a letter, only lowercase letters,
/// digits, and hyphens; no consecutive hyphens and no trailing
/// hyphen. AWS rejects mismatches with InvalidParameterValue at
/// CreateDBInstance.
fn validate_db_identifier(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 63 {
        return Err(invalid_parameter(format!(
            "DBInstanceIdentifier length must be between 1 and 63, got {}.",
            name.len()
        )));
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_alphabetic() {
        return Err(invalid_parameter(format!(
            "DBInstanceIdentifier '{name}' must start with a letter."
        )));
    }
    if name.ends_with('-') {
        return Err(invalid_parameter(format!(
            "DBInstanceIdentifier '{name}' must not end with a hyphen."
        )));
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        let is_letter = b.is_ascii_alphabetic();
        let is_digit = b.is_ascii_digit();
        let is_hyphen = b == b'-';
        if !is_letter && !is_digit && !is_hyphen {
            return Err(invalid_parameter(format!(
                "DBInstanceIdentifier '{name}' contains invalid character '{}'. \
                 Allowed: letters, digits, hyphens.",
                b as char
            )));
        }
        if is_hyphen && prev_hyphen {
            return Err(invalid_parameter(format!(
                "DBInstanceIdentifier '{name}' must not contain consecutive hyphens."
            )));
        }
        prev_hyphen = is_hyphen;
    }
    Ok(())
}

#[cfg(test)]
mod db_identifier_tests {
    use super::*;

    #[test]
    fn accepts_documented_shapes() {
        validate_db_identifier("prod").unwrap();
        validate_db_identifier("Prod-db-1").unwrap();
        validate_db_identifier("a").unwrap();
    }

    #[test]
    fn rejects_leading_digit_or_hyphen() {
        assert!(validate_db_identifier("1prod").is_err());
        assert!(validate_db_identifier("-prod").is_err());
    }

    #[test]
    fn rejects_trailing_hyphen() {
        assert!(validate_db_identifier("prod-").is_err());
    }

    #[test]
    fn rejects_consecutive_hyphens() {
        assert!(validate_db_identifier("prod--db").is_err());
    }

    #[test]
    fn rejects_disallowed_chars() {
        assert!(validate_db_identifier("prod_db").is_err());
        assert!(validate_db_identifier("prod.db").is_err());
        assert!(validate_db_identifier("prod db").is_err());
    }

    #[test]
    fn rejects_empty_and_too_long() {
        assert!(validate_db_identifier("").is_err());
        let long = "a".repeat(64);
        assert!(validate_db_identifier(&long).is_err());
    }
}

#[cfg(test)]
mod create_db_instance_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn base_input() -> Value {
        json!({
            "DBInstanceIdentifier": "prod-db",
            "DBInstanceClass": "db.t3.micro",
            "Engine": "postgres",
            "MasterUsername": "admin",
            "MasterUserPassword": "secret123",
        })
    }

    #[test]
    fn persists_iops_and_storage_throughput_on_gp3() {
        let state = RdsState::default();
        let mut input = base_input();
        input["StorageType"] = json!("gp3");
        input["Iops"] = json!(3000);
        input["StorageThroughput"] = json!(125);
        let resp = create_db_instance(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["DBInstance"]["Iops"], 3000);
        assert_eq!(resp["DBInstance"]["StorageThroughput"], 125);
    }

    #[test]
    fn rejects_iops_on_gp2() {
        let state = RdsState::default();
        let mut input = base_input();
        input["StorageType"] = json!("gp2");
        input["Iops"] = json!(3000);
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn rejects_storage_throughput_on_io1() {
        let state = RdsState::default();
        let mut input = base_input();
        input["StorageType"] = json!("io1");
        input["Iops"] = json!(3000);
        input["StorageThroughput"] = json!(125);
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn persists_vpc_security_group_ids() {
        let state = RdsState::default();
        let mut input = base_input();
        input["VpcSecurityGroupIds"] = json!(["sg-1", "sg-2"]);
        let resp = create_db_instance(&state, &input, &ctx()).unwrap();
        let sgs = resp["DBInstance"]["VpcSecurityGroups"].as_array().unwrap();
        assert_eq!(sgs.len(), 2);
        assert_eq!(sgs[0]["VpcSecurityGroupId"], "sg-1");
    }
}
