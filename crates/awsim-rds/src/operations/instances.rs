use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        db_cluster_not_found, db_instance_already_exists, db_instance_not_found,
        invalid_db_instance_state, invalid_parameter,
    },
    ids::{
        default_engine_version, default_port, instance_arn, instance_endpoint, is_aurora_engine,
        now_iso8601,
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
    if let Some(ref lm) = inst.license_model {
        obj["LicenseModel"] = json!(lm);
    }
    obj["CopyTagsToSnapshot"] = json!(inst.copy_tags_to_snapshot);
    if let Some(ref k) = inst.kms_key_id {
        obj["KmsKeyId"] = json!(k);
        obj["StorageEncrypted"] = json!(true);
    } else {
        obj["StorageEncrypted"] = json!(false);
    }
    if let Some(iv) = inst.monitoring_interval {
        obj["MonitoringInterval"] = json!(iv);
    }
    if let Some(ref role) = inst.monitoring_role_arn {
        obj["MonitoringRoleArn"] = json!(role);
    }
    obj["EnabledCloudwatchLogsExports"] = json!(inst.enabled_cloudwatch_logs_exports);
    if let Some(ref window) = inst.preferred_maintenance_window {
        obj["PreferredMaintenanceWindow"] = json!(window);
    }
    if !inst.pending_modified_values.is_empty() {
        obj["PendingModifiedValues"] =
            serde_json::to_value(&inst.pending_modified_values).unwrap_or_else(|_| json!({}));
    }
    if let Some(ref src) = inst.read_replica_source_db_instance_identifier {
        obj["ReadReplicaSourceDBInstanceIdentifier"] = json!(src);
    }
    if !inst.read_replica_db_instance_identifiers.is_empty() {
        obj["ReadReplicaDBInstanceIdentifiers"] = json!(inst.read_replica_db_instance_identifiers);
    }
    obj
}

/// Validate the AWS preferred-maintenance-window format
/// `ddd:hh24:mi-ddd:hh24:mi` (e.g. `sun:05:00-sun:06:00`). AWS
/// additionally requires the window to be at least 30 minutes wide;
/// we enforce shape and field ranges but skip the duration check
/// since clients almost universally pass the AWS-default 30-minute
/// shape.
pub(crate) fn validate_maintenance_window(s: &str) -> Result<(), AwsError> {
    let (start, end) = s.split_once('-').ok_or_else(|| {
        invalid_parameter(format!(
            "PreferredMaintenanceWindow `{s}` must be in `ddd:hh24:mi-ddd:hh24:mi` form."
        ))
    })?;
    parse_window_anchor(start, s)?;
    parse_window_anchor(end, s)?;
    Ok(())
}

fn parse_window_anchor(anchor: &str, original: &str) -> Result<(), AwsError> {
    let parts: Vec<&str> = anchor.split(':').collect();
    if parts.len() != 3 {
        return Err(invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` anchor `{anchor}` must be \
             `ddd:hh:mm`."
        )));
    }
    if !matches!(
        parts[0],
        "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
    ) {
        return Err(invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` day-of-week `{}` must be one of \
             mon/tue/wed/thu/fri/sat/sun.",
            parts[0]
        )));
    }
    let hour: u32 = parts[1].parse().map_err(|_| {
        invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` hour `{}` must be 00..=23.",
            parts[1]
        ))
    })?;
    if hour > 23 {
        return Err(invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` hour `{hour}` must be 00..=23."
        )));
    }
    let minute: u32 = parts[2].parse().map_err(|_| {
        invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` minute `{}` must be 00..=59.",
            parts[2]
        ))
    })?;
    if minute > 59 {
        return Err(invalid_parameter(format!(
            "PreferredMaintenanceWindow `{original}` minute `{minute}` must be 00..=59."
        )));
    }
    Ok(())
}

/// Default 30-minute window AWS assigns when the caller omits the
/// field. Real AWS stamps a region-specific off-hours window; we use
/// a fixed Sunday 05:00 UTC slot so tests are deterministic.
const DEFAULT_MAINTENANCE_WINDOW: &str = "sun:05:00-sun:05:30";

/// Allowed license models per engine family. AWS rejects mismatches at
/// CreateDBInstance/ModifyDBInstance with InvalidParameterCombination.
fn allowed_license_models(engine: &str) -> &'static [&'static str] {
    match engine {
        "postgres" | "mysql" | "mariadb" | "docdb" | "neptune" => &["general-public-license"],
        "sqlserver-ex" | "sqlserver-web" => &["license-included"],
        "sqlserver-se" | "sqlserver-ee" => &["license-included", "bring-your-own-license"],
        "oracle-ee" | "oracle-se" | "oracle-se1" | "oracle-se2" => {
            &["bring-your-own-license", "license-included"]
        }
        _ => &[],
    }
}

fn default_license_model(engine: &str) -> Option<&'static str> {
    allowed_license_models(engine).first().copied()
}

/// Per-engine allowlist for CloudWatch Logs exports. AWS rejects
/// out-of-range values with InvalidParameterValue.
fn allowed_log_exports(engine: &str) -> &'static [&'static str] {
    match engine {
        "mysql" | "mariadb" | "aurora" | "aurora-mysql" => {
            &["audit", "error", "general", "slowquery"]
        }
        "postgres" | "aurora-postgresql" => &["postgresql", "upgrade"],
        "oracle-ee" | "oracle-se" | "oracle-se1" | "oracle-se2" => {
            &["alert", "audit", "listener", "trace"]
        }
        "sqlserver-ex" | "sqlserver-web" | "sqlserver-se" | "sqlserver-ee" => &["agent", "error"],
        _ => &[],
    }
}

fn validate_log_export(engine: &str, log_type: &str) -> Result<(), AwsError> {
    let allowed = allowed_log_exports(engine);
    if !allowed.contains(&log_type) {
        return Err(invalid_parameter(format!(
            "Log export '{log_type}' is not valid for engine '{engine}'; allowed: {}.",
            allowed.join(", "),
        )));
    }
    Ok(())
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
    let cluster_identifier = opt_str(input, "DBClusterIdentifier").map(|s| s.to_string());
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
        _ if is_aurora_engine(engine) => {}
        _ => {
            return Err(invalid_parameter(format!("Unknown engine: {engine}")));
        }
    }

    if state.instances.contains_key(identifier) {
        return Err(db_instance_already_exists(identifier));
    }

    // Aurora instances are members of a DB cluster and inherit their
    // credentials, engine version, and storage from it. A standalone
    // instance instead carries its own master credentials and storage.
    let (master_username, engine_version, allocated_storage, storage_type) =
        if is_aurora_engine(engine) {
            let cluster_id = cluster_identifier.as_deref().ok_or_else(|| {
                invalid_parameter(format!(
                    "The engine '{engine}' requires a DBClusterIdentifier; \
                     create the DB cluster before adding instances to it."
                ))
            })?;
            let cluster = state
                .clusters
                .get(cluster_id)
                .ok_or_else(|| db_cluster_not_found(cluster_id))?;
            if cluster.engine != engine {
                return Err(invalid_parameter(format!(
                    "DB instance engine '{engine}' does not match the engine \
                     '{}' of cluster '{cluster_id}'.",
                    cluster.engine
                )));
            }
            (
                cluster.master_username.clone(),
                cluster.engine_version.clone(),
                1,
                "aurora".to_string(),
            )
        } else {
            let master_username = require_str(input, "MasterUsername")?.to_string();
            // MasterUserPassword is required by AWS but only stored, never exposed.
            let _master_password = require_str(input, "MasterUserPassword")?;
            let engine_version = opt_str(input, "EngineVersion")
                .unwrap_or_else(|| default_engine_version(engine))
                .to_string();
            (
                master_username,
                engine_version,
                opt_u32(input, "AllocatedStorage").unwrap_or(20),
                storage_type,
            )
        };

    let license_model = match opt_str(input, "LicenseModel") {
        Some(lm) => {
            let allowed = allowed_license_models(engine);
            if !allowed.contains(&lm) {
                return Err(invalid_parameter(format!(
                    "LicenseModel '{lm}' is not valid for engine '{engine}'; allowed: {}.",
                    allowed.join(", "),
                )));
            }
            Some(lm.to_string())
        }
        None => default_license_model(engine).map(str::to_string),
    };

    let arn = instance_arn(&ctx.partition, &ctx.region, &ctx.account_id, identifier);
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

    let monitoring_interval = opt_u32(input, "MonitoringInterval");
    if let Some(iv) = monitoring_interval
        && !matches!(iv, 0 | 1 | 5 | 10 | 15 | 30 | 60)
    {
        return Err(invalid_parameter(format!(
            "MonitoringInterval '{iv}' must be one of: 0, 1, 5, 10, 15, 30, 60."
        )));
    }
    let monitoring_role_arn = opt_str(input, "MonitoringRoleArn").map(str::to_string);
    if matches!(monitoring_interval, Some(iv) if iv > 0) && monitoring_role_arn.is_none() {
        return Err(invalid_parameter(
            "MonitoringRoleArn is required when MonitoringInterval is greater than 0.",
        ));
    }
    let enabled_cloudwatch_logs_exports: Vec<String> = input["EnableCloudwatchLogsExports"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for log_type in &enabled_cloudwatch_logs_exports {
        validate_log_export(engine, log_type)?;
    }

    let preferred_maintenance_window = match opt_str(input, "PreferredMaintenanceWindow") {
        Some(w) => {
            validate_maintenance_window(w)?;
            Some(w.to_string())
        }
        None => Some(DEFAULT_MAINTENANCE_WINDOW.to_string()),
    };

    let inst = DbInstance {
        identifier: identifier.to_string(),
        arn: arn.clone(),
        instance_class: instance_class.to_string(),
        engine: engine.to_string(),
        engine_version,
        status: "available".to_string(),
        master_username,
        allocated_storage,
        endpoint: Some(DbEndpoint { address, port }),
        subnet_group_name,
        vpc_security_groups,
        multi_az,
        publicly_accessible,
        storage_type,
        cluster_identifier: cluster_identifier.clone(),
        created_at: now_iso8601(),
        iops,
        storage_throughput,
        license_model,
        copy_tags_to_snapshot: opt_bool(input, "CopyTagsToSnapshot").unwrap_or(false),
        kms_key_id: opt_str(input, "KmsKeyId").map(str::to_string),
        monitoring_interval,
        monitoring_role_arn,
        enabled_cloudwatch_logs_exports,
        preferred_maintenance_window,
        pending_modified_values: std::collections::HashMap::new(),
        read_replica_source_db_instance_identifier: None,
        read_replica_db_instance_identifiers: Vec::new(),
    };

    let result = instance_to_value(&inst);

    // Register the instance as a member of its cluster. Membership order
    // determines roles: the first instance to join is the writer and the
    // rest are read replicas (see `cluster_to_value`).
    if let Some(ref cluster_id) = cluster_identifier
        && let Some(mut cluster) = state.clusters.get_mut(cluster_id)
        && !cluster.members.iter().any(|m| m == identifier)
    {
        cluster.members.push(identifier.to_string());
    }

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

    // Match AWS: a source with attached read replicas refuses
    // DeleteDBInstance until each replica is deleted (or promoted)
    // first. The error mirrors the documented `InvalidDBInstanceState`.
    if !inst.read_replica_db_instance_identifiers.is_empty() {
        return Err(invalid_db_instance_state(
            identifier,
            &format!(
                "{} has {} read replica(s); delete or promote them before \
                 deleting the source.",
                identifier,
                inst.read_replica_db_instance_identifiers.len(),
            ),
        ));
    }

    let source = inst.read_replica_source_db_instance_identifier.clone();
    let cluster_identifier = inst.cluster_identifier.clone();

    let result = instance_to_value(&inst);
    drop(inst);
    state.instances.remove(identifier);

    // Unlink this replica from its source's child list. We do this
    // *after* the remove so the read-replica delete is fully
    // observable when DescribeDBInstances next reads the source.
    if let Some(ref src) = source
        && let Some(mut src_inst) = state.instances.get_mut(src)
    {
        src_inst
            .read_replica_db_instance_identifiers
            .retain(|id| id != identifier);
    }

    // Drop the instance from its cluster's member list. Removing the
    // first member (the writer) promotes the next member to writer,
    // since membership order is preserved.
    if let Some(ref cluster_id) = cluster_identifier
        && let Some(mut cluster) = state.clusters.get_mut(cluster_id)
    {
        cluster.members.retain(|m| m != identifier);
    }

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

    let max_records = cap_max_results(input["MaxRecords"].as_i64(), 100, 100);
    let mut items: Vec<(String, Value)> = state
        .instances
        .iter()
        .map(|e| (e.key().clone(), instance_to_value(e.value())))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let page = paginate(items, max_records, opt_str(input, "Marker"), |(k, _)| {
        k.clone()
    })?;
    let db_instances: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    Ok(json!({
        "DBInstances": { "DBInstance": db_instances },
        "Marker": page.next_token,
    }))
}

/// Create a new DB instance that follows another instance as a read
/// replica. AWS's `CreateDBInstanceReadReplica` clones most of the
/// source's surface (engine, class, storage type) and stamps a
/// `ReadReplicaSourceDBInstanceIdentifier` on the new row. The
/// source instance's `ReadReplicaDBInstanceIdentifiers` is updated
/// to include the new replica so describe surfaces it.
pub fn create_db_instance_read_replica(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBInstanceIdentifier")?;
    validate_db_identifier(identifier)?;
    let source_identifier = require_str(input, "SourceDBInstanceIdentifier")?;

    if state.instances.contains_key(identifier) {
        return Err(db_instance_already_exists(identifier));
    }

    // Clone the source's surface up front; the source must exist and
    // must not itself be a replica (AWS rejects cascading replicas
    // with InvalidDBInstanceState).
    let source = state
        .instances
        .get(source_identifier)
        .ok_or_else(|| db_instance_not_found(source_identifier))?
        .clone();
    if source.read_replica_source_db_instance_identifier.is_some() {
        return Err(invalid_db_instance_state(
            source_identifier,
            "Read replicas cannot themselves serve as the source of another \
             read replica.",
        ));
    }

    // The replica inherits the source's engine, class, storage, etc.,
    // but takes a fresh ARN/endpoint and an optional instance-class
    // override.
    let instance_class = opt_str(input, "DBInstanceClass")
        .unwrap_or(&source.instance_class)
        .to_string();
    let arn = instance_arn(&ctx.partition, &ctx.region, &ctx.account_id, identifier);
    let address = instance_endpoint(identifier, &ctx.region);
    let port = default_port(&source.engine);

    let replica = DbInstance {
        identifier: identifier.to_string(),
        arn: arn.clone(),
        instance_class,
        engine: source.engine.clone(),
        engine_version: source.engine_version.clone(),
        status: "available".to_string(),
        master_username: source.master_username.clone(),
        allocated_storage: source.allocated_storage,
        endpoint: Some(DbEndpoint { address, port }),
        subnet_group_name: source.subnet_group_name.clone(),
        vpc_security_groups: source.vpc_security_groups.clone(),
        multi_az: false,
        publicly_accessible: opt_bool(input, "PubliclyAccessible").unwrap_or(false),
        storage_type: source.storage_type.clone(),
        cluster_identifier: None,
        created_at: now_iso8601(),
        iops: source.iops,
        storage_throughput: source.storage_throughput,
        license_model: source.license_model.clone(),
        copy_tags_to_snapshot: opt_bool(input, "CopyTagsToSnapshot")
            .unwrap_or(source.copy_tags_to_snapshot),
        kms_key_id: opt_str(input, "KmsKeyId")
            .map(str::to_string)
            .or(source.kms_key_id.clone()),
        monitoring_interval: source.monitoring_interval,
        monitoring_role_arn: source.monitoring_role_arn.clone(),
        enabled_cloudwatch_logs_exports: source.enabled_cloudwatch_logs_exports.clone(),
        preferred_maintenance_window: Some(DEFAULT_MAINTENANCE_WINDOW.to_string()),
        pending_modified_values: std::collections::HashMap::new(),
        read_replica_source_db_instance_identifier: Some(source_identifier.to_string()),
        read_replica_db_instance_identifiers: Vec::new(),
    };

    let result = instance_to_value(&replica);
    state.instances.insert(identifier.to_string(), replica);

    // Push the replica identifier onto the source's list so describes
    // of the source surface it.
    if let Some(mut src) = state.instances.get_mut(source_identifier) {
        src.read_replica_db_instance_identifiers
            .push(identifier.to_string());
    }

    Ok(json!({ "DBInstance": result }))
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

    // PreferredMaintenanceWindow applies immediately on real AWS — it
    // controls *when* future changes flush, so staging it under
    // PendingModifiedValues would be self-defeating. Validate shape
    // up-front so a bad string surfaces InvalidParameterValue rather
    // than silently corrupting the schedule.
    if let Some(window) = opt_str(input, "PreferredMaintenanceWindow") {
        validate_maintenance_window(window)?;
        inst.preferred_maintenance_window = Some(window.to_string());
    }

    let apply_immediately = opt_bool(input, "ApplyImmediately").unwrap_or(false);

    // Validate LicenseModel before deciding the apply path so a bad
    // value surfaces as InvalidParameterValue regardless of timing.
    let validated_license_model = match opt_str(input, "LicenseModel") {
        Some(lm) => {
            let allowed = allowed_license_models(&inst.engine);
            if !allowed.contains(&lm) {
                return Err(invalid_parameter(format!(
                    "LicenseModel '{lm}' is not valid for engine '{}'; allowed: {}.",
                    inst.engine,
                    allowed.join(", "),
                )));
            }
            Some(lm.to_string())
        }
        None => None,
    };

    if apply_immediately {
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
        if let Some(lm) = validated_license_model {
            inst.license_model = Some(lm);
        }
        inst.pending_modified_values.clear();
    } else {
        // Stage the diff. AWS shape: PendingModifiedValues echoes the
        // requested *new* values; the live config keeps the current
        // ones until the next maintenance window applies them.
        if let Some(class) = opt_str(input, "DBInstanceClass") {
            inst.pending_modified_values
                .insert("DBInstanceClass".to_string(), json!(class));
        }
        if let Some(storage) = opt_u32(input, "AllocatedStorage") {
            inst.pending_modified_values
                .insert("AllocatedStorage".to_string(), json!(storage));
        }
        if let Some(multi_az) = opt_bool(input, "MultiAZ") {
            inst.pending_modified_values
                .insert("MultiAZ".to_string(), json!(multi_az));
        }
        if let Some(publicly_accessible) = opt_bool(input, "PubliclyAccessible") {
            inst.pending_modified_values
                .insert("PubliclyAccessible".to_string(), json!(publicly_accessible));
        }
        if let Some(storage_type) = opt_str(input, "StorageType") {
            inst.pending_modified_values
                .insert("StorageType".to_string(), json!(storage_type));
        }
        if let Some(lm) = validated_license_model {
            inst.pending_modified_values
                .insert("LicenseModel".to_string(), json!(lm));
        }
    }

    let result = instance_to_value(&inst);
    Ok(json!({ "DBInstance": result }))
}

/// Flush every staged key in `pending_modified_values` back onto the
/// live `DbInstance` fields, then clear the map. Mirrors the immediate
/// apply path in [`modify_db_instance`]: this is what the maintenance
/// window runs when `ApplyImmediately` was false. Pure and
/// wall-clock-free so it can be unit-tested in isolation; the tick
/// driver decides *when* to call it.
pub fn apply_pending_modified_values(instance: &mut DbInstance) {
    if instance.pending_modified_values.is_empty() {
        return;
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("DBInstanceClass")
        .and_then(|v| v.as_str())
    {
        instance.instance_class = v.to_string();
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("AllocatedStorage")
        .and_then(|v| v.as_u64())
    {
        instance.allocated_storage = v as u32;
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("MultiAZ")
        .and_then(|v| v.as_bool())
    {
        instance.multi_az = v;
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("PubliclyAccessible")
        .and_then(|v| v.as_bool())
    {
        instance.publicly_accessible = v;
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("StorageType")
        .and_then(|v| v.as_str())
    {
        instance.storage_type = v.to_string();
    }
    if let Some(v) = instance
        .pending_modified_values
        .get("LicenseModel")
        .and_then(|v| v.as_str())
    {
        instance.license_model = Some(v.to_string());
    }
    instance.pending_modified_values.clear();
}

/// Whether `now` falls inside the AWS preferred-maintenance-window
/// `window` (`ddd:hh24:mi-ddd:hh24:mi`). We compare against the
/// window's *start* anchor at minute granularity: a match means the
/// current weekday/hour/minute equals the start of the window, which
/// is when the tick driver flushes staged changes. Malformed windows
/// never match. No chrono: the day/hour/minute are derived from the
/// unix timestamp with the same arithmetic style as the rest of the
/// crate (unix epoch 1970-01-01 was a Thursday).
pub fn maintenance_window_matches(window: &str, now: SystemTime) -> bool {
    let Some((start, _end)) = window.split_once('-') else {
        return false;
    };
    let parts: Vec<&str> = start.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let Some(start_day) = day_index(parts[0]) else {
        return false;
    };
    let (Ok(start_hour), Ok(start_minute)) = (parts[1].parse::<u64>(), parts[2].parse::<u64>())
    else {
        return false;
    };

    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let minute = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let days = secs / 86_400;
    // 1970-01-01 was a Thursday; mon=0..sun=6 to match `day_index`.
    let weekday = ((days % 7) + 3) % 7;

    weekday == start_day && hour == start_hour && minute == start_minute
}

/// Map an AWS day-of-week abbreviation to a Monday-based index
/// (mon=0..sun=6). Returns `None` for anything outside the set.
fn day_index(day: &str) -> Option<u64> {
    match day {
        "mon" => Some(0),
        "tue" => Some(1),
        "wed" => Some(2),
        "thu" => Some(3),
        "fri" => Some(4),
        "sat" => Some(5),
        "sun" => Some(6),
        _ => None,
    }
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
    fn describe_db_instances_paginates() {
        let state = RdsState::default();
        for id in ["a-db", "b-db", "c-db"] {
            let mut input = base_input();
            input["DBInstanceIdentifier"] = json!(id);
            create_db_instance(&state, &input, &ctx()).unwrap();
        }

        let mut seen: Vec<String> = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let mut input = json!({ "MaxRecords": 2 });
            if let Some(m) = &marker {
                input["Marker"] = json!(m);
            }
            let resp = describe_db_instances(&state, &input, &ctx()).unwrap();
            for inst in resp["DBInstances"]["DBInstance"].as_array().unwrap() {
                seen.push(inst["DBInstanceIdentifier"].as_str().unwrap().to_string());
            }
            match resp["Marker"].as_str() {
                Some(m) => marker = Some(m.to_string()),
                None => break,
            }
        }
        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            3,
            "every instance returned exactly once across pages"
        );
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
    fn defaults_license_model_per_engine() {
        let state = RdsState::default();
        let resp = create_db_instance(&state, &base_input(), &ctx()).unwrap();
        assert_eq!(resp["DBInstance"]["LicenseModel"], "general-public-license");
    }

    #[test]
    fn rejects_license_model_not_valid_for_engine() {
        let state = RdsState::default();
        let mut input = base_input();
        // postgres only allows general-public-license.
        input["LicenseModel"] = json!("bring-your-own-license");
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("LicenseModel"));
    }

    #[test]
    fn accepts_byol_for_sqlserver_se() {
        let state = RdsState::default();
        let mut input = base_input();
        input["DBInstanceIdentifier"] = json!("sql-db");
        input["Engine"] = json!("sqlserver-se");
        input["LicenseModel"] = json!("bring-your-own-license");
        let resp = create_db_instance(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["DBInstance"]["LicenseModel"], "bring-your-own-license");
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

    #[test]
    fn persists_enhanced_monitoring_and_log_exports() {
        let state = RdsState::default();
        let mut input = base_input();
        input["MonitoringInterval"] = json!(60);
        input["MonitoringRoleArn"] = json!("arn:aws:iam::123:role/rds-monitoring");
        input["EnableCloudwatchLogsExports"] = json!(["postgresql", "upgrade"]);
        let resp = create_db_instance(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["DBInstance"]["MonitoringInterval"], 60);
        assert_eq!(
            resp["DBInstance"]["MonitoringRoleArn"],
            "arn:aws:iam::123:role/rds-monitoring"
        );
        let exports = resp["DBInstance"]["EnabledCloudwatchLogsExports"]
            .as_array()
            .unwrap();
        assert_eq!(exports.len(), 2);
    }

    #[test]
    fn rejects_monitoring_interval_not_in_allowed_set() {
        let state = RdsState::default();
        let mut input = base_input();
        input["MonitoringInterval"] = json!(7);
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("MonitoringInterval"));
    }

    #[test]
    fn rejects_monitoring_without_role_arn() {
        let state = RdsState::default();
        let mut input = base_input();
        input["MonitoringInterval"] = json!(60);
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("MonitoringRoleArn"));
    }

    #[test]
    fn rejects_log_export_not_valid_for_engine() {
        let state = RdsState::default();
        let mut input = base_input();
        // postgres does not have a "slowquery" log type.
        input["EnableCloudwatchLogsExports"] = json!(["slowquery"]);
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert!(err.message.contains("slowquery"));
    }

    #[test]
    fn stamps_default_maintenance_window_when_omitted() {
        let state = RdsState::default();
        let resp = create_db_instance(&state, &base_input(), &ctx()).unwrap();
        assert_eq!(
            resp["DBInstance"]["PreferredMaintenanceWindow"],
            json!(DEFAULT_MAINTENANCE_WINDOW)
        );
    }

    #[test]
    fn accepts_custom_maintenance_window() {
        let state = RdsState::default();
        let mut input = base_input();
        input["PreferredMaintenanceWindow"] = json!("tue:03:30-tue:04:00");
        let resp = create_db_instance(&state, &input, &ctx()).unwrap();
        assert_eq!(
            resp["DBInstance"]["PreferredMaintenanceWindow"],
            json!("tue:03:30-tue:04:00")
        );
    }

    #[test]
    fn rejects_malformed_maintenance_window() {
        for bad in [
            "garbage",
            "sun:99:00-sun:09:30",
            "sun:05:00-funday:06:00",
            "sun:05:00",
        ] {
            let state = RdsState::default();
            let mut input = base_input();
            input["PreferredMaintenanceWindow"] = json!(bad);
            let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
            assert_eq!(err.code, "InvalidParameterValue", "input `{bad}`");
        }
    }
}

#[cfg(test)]
mod modify_db_instance_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn seed(state: &RdsState) {
        let input = json!({
            "DBInstanceIdentifier": "prod-db",
            "DBInstanceClass": "db.t3.micro",
            "Engine": "postgres",
            "MasterUsername": "admin",
            "MasterUserPassword": "secret123",
        });
        create_db_instance(state, &input, &ctx()).unwrap();
    }

    #[test]
    fn apply_immediately_false_stages_diff_under_pending_modified_values() {
        let state = RdsState::default();
        seed(&state);
        let resp = modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "DBInstanceClass": "db.t3.large",
                "AllocatedStorage": 200,
            }),
            &ctx(),
        )
        .unwrap();
        // Live config still reflects the original class until the
        // window applies; the new class lives under PendingModifiedValues.
        assert_eq!(resp["DBInstance"]["DBInstanceClass"], json!("db.t3.micro"));
        assert_eq!(resp["DBInstance"]["AllocatedStorage"], json!(20));
        assert_eq!(
            resp["DBInstance"]["PendingModifiedValues"]["DBInstanceClass"],
            json!("db.t3.large")
        );
        assert_eq!(
            resp["DBInstance"]["PendingModifiedValues"]["AllocatedStorage"],
            json!(200)
        );
    }

    #[test]
    fn apply_immediately_true_flushes_live_and_clears_pending() {
        let state = RdsState::default();
        seed(&state);
        let resp = modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "DBInstanceClass": "db.t3.large",
                "ApplyImmediately": true,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["DBInstance"]["DBInstanceClass"], json!("db.t3.large"));
        assert!(
            resp["DBInstance"].get("PendingModifiedValues").is_none(),
            "PendingModifiedValues should be omitted when no diff is staged: {resp}"
        );
    }

    #[test]
    fn modify_maintenance_window_applies_immediately() {
        let state = RdsState::default();
        seed(&state);
        let resp = modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "PreferredMaintenanceWindow": "sat:10:00-sat:10:30",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            resp["DBInstance"]["PreferredMaintenanceWindow"],
            json!("sat:10:00-sat:10:30")
        );
    }

    #[test]
    fn create_read_replica_links_source_and_replica() {
        let state = RdsState::default();
        seed(&state);
        let resp = create_db_instance_read_replica(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db-ro",
                "SourceDBInstanceIdentifier": "prod-db",
            }),
            &ctx(),
        )
        .unwrap();
        // The new replica exposes the source on describe.
        assert_eq!(
            resp["DBInstance"]["ReadReplicaSourceDBInstanceIdentifier"],
            json!("prod-db")
        );
        // The source's child list now includes the replica.
        let src = state.instances.get("prod-db").unwrap();
        assert_eq!(src.read_replica_db_instance_identifiers, vec!["prod-db-ro"]);
    }

    #[test]
    fn delete_replica_unlinks_from_source() {
        let state = RdsState::default();
        seed(&state);
        create_db_instance_read_replica(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db-ro",
                "SourceDBInstanceIdentifier": "prod-db",
            }),
            &ctx(),
        )
        .unwrap();
        delete_db_instance(
            &state,
            &json!({ "DBInstanceIdentifier": "prod-db-ro" }),
            &ctx(),
        )
        .unwrap();
        let src = state.instances.get("prod-db").unwrap();
        assert!(src.read_replica_db_instance_identifiers.is_empty());
    }

    #[test]
    fn delete_source_with_attached_replicas_is_rejected() {
        let state = RdsState::default();
        seed(&state);
        create_db_instance_read_replica(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db-ro",
                "SourceDBInstanceIdentifier": "prod-db",
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_db_instance(
            &state,
            &json!({ "DBInstanceIdentifier": "prod-db" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidDBInstanceState");
    }

    #[test]
    fn replica_of_replica_is_rejected() {
        let state = RdsState::default();
        seed(&state);
        create_db_instance_read_replica(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db-ro",
                "SourceDBInstanceIdentifier": "prod-db",
            }),
            &ctx(),
        )
        .unwrap();
        let err = create_db_instance_read_replica(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db-ro-ro",
                "SourceDBInstanceIdentifier": "prod-db-ro",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidDBInstanceState");
    }

    #[test]
    fn modify_rejects_malformed_maintenance_window() {
        let state = RdsState::default();
        seed(&state);
        let err = modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "PreferredMaintenanceWindow": "sometime",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn apply_pending_modified_values_flushes_staged_diff() {
        let state = RdsState::default();
        seed(&state);
        // Stage a diff with ApplyImmediately=false.
        modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "DBInstanceClass": "db.t3.large",
                "AllocatedStorage": 200,
            }),
            &ctx(),
        )
        .unwrap();
        // The window applying the change calls the pure helper directly.
        let mut inst = state.instances.get_mut("prod-db").unwrap();
        apply_pending_modified_values(&mut inst);
        assert_eq!(inst.instance_class, "db.t3.large");
        assert_eq!(inst.allocated_storage, 200);
        assert!(inst.pending_modified_values.is_empty());
    }

    #[test]
    fn maintenance_window_non_match_leaves_pending_intact() {
        let state = RdsState::default();
        seed(&state);
        modify_db_instance(
            &state,
            &json!({
                "DBInstanceIdentifier": "prod-db",
                "DBInstanceClass": "db.t3.large",
            }),
            &ctx(),
        )
        .unwrap();
        // A window the clock is not currently inside must not apply.
        // UNIX_EPOCH is Thursday 00:00, so a Monday-00:00 window can
        // never match at that instant.
        assert!(!maintenance_window_matches(
            "mon:00:00-mon:00:30",
            UNIX_EPOCH
        ));
        // Guard: nothing flushed because the tick gate stayed closed.
        let inst = state.instances.get("prod-db").unwrap();
        assert_eq!(inst.instance_class, "db.t3.micro");
        assert_eq!(
            inst.pending_modified_values.get("DBInstanceClass"),
            Some(&json!("db.t3.large"))
        );
    }

    #[test]
    fn maintenance_window_matches_start_anchor() {
        // UNIX_EPOCH itself is Thursday 00:00 UTC.
        assert!(maintenance_window_matches(
            "thu:00:00-thu:00:30",
            UNIX_EPOCH
        ));
        // Wrong weekday at the same wall-clock minute does not match.
        assert!(!maintenance_window_matches(
            "wed:00:00-wed:00:30",
            UNIX_EPOCH
        ));
        // Malformed windows never match.
        assert!(!maintenance_window_matches("garbage", UNIX_EPOCH));
    }
}

#[cfg(test)]
mod aurora_membership_tests {
    use super::*;
    use crate::operations::clusters::{create_db_cluster, describe_db_clusters};

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_cluster(state: &RdsState, id: &str) {
        let input = json!({
            "DBClusterIdentifier": id,
            "Engine": "aurora-postgresql",
            "EngineVersion": "15.4",
            "MasterUsername": "clusteradmin",
            "MasterUserPassword": "secret123",
        });
        create_db_cluster(state, &input, &ctx()).unwrap();
    }

    fn add_instance(state: &RdsState, instance_id: &str, cluster_id: &str) -> Value {
        let input = json!({
            "DBInstanceIdentifier": instance_id,
            "DBInstanceClass": "db.r6g.large",
            "Engine": "aurora-postgresql",
            "DBClusterIdentifier": cluster_id,
        });
        create_db_instance(state, &input, &ctx()).unwrap()
    }

    fn members(state: &RdsState, cluster_id: &str) -> Vec<Value> {
        let resp =
            describe_db_clusters(state, &json!({ "DBClusterIdentifier": cluster_id }), &ctx())
                .unwrap();
        resp["DBClusters"]["DBCluster"][0]["DBClusterMembers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn first_instance_joins_as_writer() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        add_instance(&state, "aurora-pg-1", "aurora-pg");

        let members = members(&state, "aurora-pg");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["DBInstanceIdentifier"], "aurora-pg-1");
        assert_eq!(members[0]["IsClusterWriter"], true);
    }

    #[test]
    fn second_instance_joins_as_reader() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        add_instance(&state, "aurora-pg-1", "aurora-pg");
        add_instance(&state, "aurora-pg-2", "aurora-pg");

        let members = members(&state, "aurora-pg");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["IsClusterWriter"], true);
        assert_eq!(members[1]["DBInstanceIdentifier"], "aurora-pg-2");
        assert_eq!(members[1]["IsClusterWriter"], false);
    }

    #[test]
    fn deleting_writer_promotes_next_member() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        add_instance(&state, "aurora-pg-1", "aurora-pg");
        add_instance(&state, "aurora-pg-2", "aurora-pg");

        delete_db_instance(
            &state,
            &json!({ "DBInstanceIdentifier": "aurora-pg-1" }),
            &ctx(),
        )
        .unwrap();

        let members = members(&state, "aurora-pg");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["DBInstanceIdentifier"], "aurora-pg-2");
        assert_eq!(members[0]["IsClusterWriter"], true);
    }

    #[test]
    fn instance_inherits_cluster_master_username_and_version() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        let resp = add_instance(&state, "aurora-pg-1", "aurora-pg");

        assert_eq!(resp["DBInstance"]["MasterUsername"], "clusteradmin");
        assert_eq!(resp["DBInstance"]["EngineVersion"], "15.4");
        assert_eq!(resp["DBInstance"]["StorageType"], "aurora");
    }

    #[test]
    fn aurora_instance_requires_a_cluster() {
        let state = RdsState::default();
        let input = json!({
            "DBInstanceIdentifier": "orphan",
            "DBInstanceClass": "db.r6g.large",
            "Engine": "aurora-mysql",
        });
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn aurora_instance_rejects_unknown_cluster() {
        let state = RdsState::default();
        let input = json!({
            "DBInstanceIdentifier": "ghost",
            "DBInstanceClass": "db.r6g.large",
            "Engine": "aurora-postgresql",
            "DBClusterIdentifier": "missing-cluster",
        });
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "DBClusterNotFoundFault");
    }

    #[test]
    fn aurora_instance_engine_must_match_cluster() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        let input = json!({
            "DBInstanceIdentifier": "mismatch",
            "DBInstanceClass": "db.r6g.large",
            "Engine": "aurora-mysql",
            "DBClusterIdentifier": "aurora-pg",
        });
        let err = create_db_instance(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }
}
