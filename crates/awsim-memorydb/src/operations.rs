use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Acl, Cluster, MemoryDbState, ParameterGroup, Snapshot, SubnetGroup, User};

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValueException",
            format!("{key} is required"),
        )
    })
}

fn arn(ctx: &RequestContext, kind: &str, name: &str) -> String {
    format!(
        "arn:aws:memorydb:{}:{}:{}/{}",
        ctx.region, ctx.account_id, kind, name
    )
}

const VALID_DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let hh: u8 = h.parse().ok()?;
    let mm: u8 = m.parse().ok()?;
    if hh > 23 || mm > 59 {
        return None;
    }
    Some((hh, mm))
}

/// Validates a MemoryDB `MaintenanceWindow` of the form
/// `ddd:hh24:mi-ddd:hh24:mi` (e.g. `sun:23:00-mon:01:30`). Days must
/// be one of {sun,mon,tue,wed,thu,fri,sat}; start and end may not be
/// equal.
fn validate_maintenance_window(s: &str) -> Result<(), AwsError> {
    let lower = s.to_ascii_lowercase();
    let (start, end) = lower.split_once('-').ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValueException",
            format!("MaintenanceWindow `{s}` must be `ddd:hh:mm-ddd:hh:mm`."),
        )
    })?;
    let parse = |part: &str| -> Option<(&'static str, u8, u8)> {
        let mut it = part.splitn(3, ':');
        let day = it.next()?;
        let hh = it.next()?;
        let mm = it.next()?;
        if it.next().is_some() {
            return None;
        }
        let day_idx = VALID_DAYS.iter().position(|d| *d == day)?;
        let (h, m) = parse_hhmm(&format!("{hh}:{mm}"))?;
        Some((VALID_DAYS[day_idx], h, m))
    };
    let s_parts = parse(start);
    let e_parts = parse(end);
    match (s_parts, e_parts) {
        (Some(a), Some(b)) if a != b => Ok(()),
        _ => Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("MaintenanceWindow `{s}` is malformed."),
        )),
    }
}

/// Validates a MemoryDB `SnapshotWindow` of the form `hh24:mi-hh24:mi`
/// (e.g. `03:00-04:00`). Start and end must be valid 24-hour clock
/// times and not equal.
fn validate_snapshot_window(s: &str) -> Result<(), AwsError> {
    let (start, end) = s.split_once('-').ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterValueException",
            format!("SnapshotWindow `{s}` must be `hh:mm-hh:mm`."),
        )
    })?;
    match (parse_hhmm(start), parse_hhmm(end)) {
        (Some(a), Some(b)) if a != b => Ok(()),
        _ => Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("SnapshotWindow `{s}` is malformed."),
        )),
    }
}

/// AWS-published MemoryDB engine versions and their newest patch
/// release. Mirrors `aws memorydb describe-engine-versions` output for
/// the supported control-plane engine identifiers.
const ENGINE_PATCH_VERSIONS: &[(&str, &str)] =
    &[("7.1", "7.1.0"), ("7.0", "7.0.7"), ("6.2", "6.2.6")];

fn engine_patch_version_for(engine_version: &str) -> &'static str {
    ENGINE_PATCH_VERSIONS
        .iter()
        .find(|(v, _)| *v == engine_version)
        .map(|(_, p)| *p)
        .unwrap_or("7.1.0")
}

fn cluster_to_value(c: &Cluster) -> Value {
    json!({
        "Name": c.name,
        "ARN": c.arn,
        "Status": c.status,
        "NodeType": c.node_type,
        "EngineVersion": c.engine_version,
        "EnginePatchVersion": c.engine_patch_version,
        "ParameterGroupName": c.parameter_group_name,
        "ParameterGroupStatus": c.parameter_group_status,
        "SubnetGroupName": c.subnet_group_name,
        "SecurityGroups": c.security_group_ids.iter().map(|id| json!({ "SecurityGroupId": id, "Status": "active" })).collect::<Vec<_>>(),
        "ACLName": c.acl_name,
        "AutoMinorVersionUpgrade": c.auto_minor_version_upgrade,
        "ClusterEndpoint": c.cluster_endpoint,
        "NumberOfShards": c.number_of_shards,
        "TLSEnabled": c.tls_enabled,
        "KmsKeyId": c.kms_key_id,
        "MaintenanceWindow": c.maintenance_window,
        "SnapshotRetentionLimit": c.snapshot_retention_limit,
        "SnapshotWindow": c.snapshot_window,
        "SnsTopicArn": c.sns_topic_arn,
        "SnsTopicStatus": c.sns_topic_status,
        "Description": c.description,
        "DataTiering": if c.data_tiering { "true" } else { "false" },
        "NetworkType": c.network_type,
        "IpDiscovery": c.ip_discovery,
        "Shards": [],
    })
}

fn user_to_value(u: &User) -> Value {
    json!({
        "Name": u.name,
        "ARN": u.arn,
        "Status": u.status,
        "AccessString": u.access_string,
        "MinimumEngineVersion": u.minimum_engine_version,
        "Authentication": { "Type": u.authentication_mode, "PasswordCount": 0 },
        "ACLNames": [],
    })
}

fn acl_to_value(a: &Acl) -> Value {
    json!({
        "Name": a.name,
        "ARN": a.arn,
        "Status": a.status,
        "UserNames": a.user_names,
        "MinimumEngineVersion": a.minimum_engine_version,
        "Clusters": [],
    })
}

pub fn create_cluster(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ClusterName")?.to_string();
    if state.clusters.contains_key(&name) {
        return Err(AwsError::conflict(
            "ClusterAlreadyExistsFault",
            format!("Cluster {name} already exists"),
        ));
    }
    // AWS MemoryDB caps NumShards at 1..=500 and NumReplicasPerShard
    // at 0..=5. Reject anything outside before allocating state.
    if let Some(n) = input.get("NumShards").and_then(Value::as_i64)
        && !(1..=500).contains(&n)
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("NumShards `{n}` must be in 1..=500."),
        ));
    }
    if let Some(n) = input.get("NumReplicasPerShard").and_then(Value::as_i64)
        && !(0..=5).contains(&n)
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("NumReplicasPerShard `{n}` must be in 0..=5."),
        ));
    }
    let node_type = require_str(input, "NodeType")?.to_string();
    let data_tiering = input
        .get("DataTiering")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if data_tiering && !node_type.starts_with("db.r6gd.") {
        return Err(AwsError::bad_request(
            "InvalidParameterCombinationException",
            format!("DataTiering=true requires a `db.r6gd.*` node type; got `{node_type}`.",),
        ));
    }
    if let Some(mw) = input.get("MaintenanceWindow").and_then(Value::as_str) {
        validate_maintenance_window(mw)?;
    }
    if let Some(sw) = input.get("SnapshotWindow").and_then(Value::as_str) {
        validate_snapshot_window(sw)?;
    }
    let network_type = match input.get("NetworkType").and_then(Value::as_str) {
        Some(v) => {
            let lower = v.to_ascii_lowercase();
            if !["ipv4", "ipv6", "dual_stack"].contains(&lower.as_str()) {
                return Err(AwsError::bad_request(
                    "InvalidParameterValueException",
                    format!("NetworkType `{v}` must be one of ipv4, ipv6, dual_stack."),
                ));
            }
            lower
        }
        None => "ipv4".to_string(),
    };
    let ip_discovery = match input.get("IpDiscovery").and_then(Value::as_str) {
        Some(v) => {
            let lower = v.to_ascii_lowercase();
            if !["ipv4", "ipv6"].contains(&lower.as_str()) {
                return Err(AwsError::bad_request(
                    "InvalidParameterValueException",
                    format!("IpDiscovery `{v}` must be one of ipv4, ipv6."),
                ));
            }
            lower
        }
        None => "ipv4".to_string(),
    };
    if ip_discovery == "ipv6" && network_type == "ipv4" {
        return Err(AwsError::bad_request(
            "InvalidParameterCombinationException",
            "IpDiscovery=ipv6 requires NetworkType in {ipv6, dual_stack}.".to_string(),
        ));
    }
    let acl_name = require_str(input, "ACLName")?.to_string();
    let arn_str = arn(ctx, "cluster", &name);
    let endpoint = json!({
        "Address": format!("clustercfg.{}.{}.memorydb.amazonaws.com", name, ctx.region),
        "Port": 6379,
    });
    let c = Cluster {
        name: name.clone(),
        arn: arn_str.clone(),
        status: "available".to_string(),
        node_type,
        engine_version: input
            .get("EngineVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("7.1")
            .to_string(),
        engine_patch_version: engine_patch_version_for(
            input
                .get("EngineVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("7.1"),
        )
        .to_string(),
        parameter_group_name: input
            .get("ParameterGroupName")
            .and_then(|v| v.as_str())
            .unwrap_or("default.memorydb-redis7")
            .to_string(),
        parameter_group_status: "in-sync".to_string(),
        subnet_group_name: input
            .get("SubnetGroupName")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string(),
        security_group_ids: input
            .get("SecurityGroupIds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        acl_name,
        auto_minor_version_upgrade: input
            .get("AutoMinorVersionUpgrade")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        cluster_endpoint: endpoint,
        number_of_shards: input
            .get("NumShards")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(1),
        tls_enabled: input
            .get("TLSEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        kms_key_id: input
            .get("KmsKeyId")
            .and_then(|v| v.as_str())
            .map(String::from),
        maintenance_window: input
            .get("MaintenanceWindow")
            .and_then(|v| v.as_str())
            .unwrap_or("sun:23:00-mon:01:30")
            .to_string(),
        snapshot_retention_limit: input
            .get("SnapshotRetentionLimit")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0),
        snapshot_window: input
            .get("SnapshotWindow")
            .and_then(|v| v.as_str())
            .unwrap_or("03:00-04:00")
            .to_string(),
        sns_topic_arn: input
            .get("SnsTopicArn")
            .and_then(|v| v.as_str())
            .map(String::from),
        sns_topic_status: "active".to_string(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        data_tiering,
        network_type,
        ip_discovery,
    };
    let result = json!({ "Cluster": cluster_to_value(&c) });
    state.clusters.insert(name, c);
    Ok(result)
}

pub fn describe_clusters(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input.get("ClusterName").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .clusters
        .iter()
        .filter(|e| match name_filter {
            Some(n) => e.value().name == n,
            None => true,
        })
        .map(|e| cluster_to_value(e.value()))
        .collect();
    Ok(json!({ "Clusters": items }))
}

pub fn delete_cluster(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ClusterName")?;
    let (_, c) = state.clusters.remove(name).ok_or_else(|| {
        AwsError::not_found("ClusterNotFoundFault", format!("Cluster {name} not found"))
    })?;
    Ok(json!({ "Cluster": cluster_to_value(&c) }))
}

pub fn update_cluster(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ClusterName")?;
    let mut c = state.clusters.get_mut(name).ok_or_else(|| {
        AwsError::not_found("ClusterNotFoundFault", format!("Cluster {name} not found"))
    })?;
    if let Some(nt) = input.get("NodeType").and_then(|v| v.as_str()) {
        c.node_type = nt.to_string();
    }
    if let Some(ev) = input.get("EngineVersion").and_then(|v| v.as_str()) {
        c.engine_version = ev.to_string();
        c.engine_patch_version = engine_patch_version_for(ev).to_string();
    }
    if let Some(d) = input.get("Description").and_then(|v| v.as_str()) {
        c.description = Some(d.to_string());
    }
    Ok(json!({ "Cluster": cluster_to_value(&c) }))
}

pub fn create_user(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "UserName")?.to_string();
    if state.users.contains_key(&name) {
        return Err(AwsError::conflict(
            "UserAlreadyExistsFault",
            format!("User {name} already exists"),
        ));
    }
    let access = require_str(input, "AccessString")?.to_string();
    let auth_type = input
        .get("AuthenticationMode")
        .and_then(|m| m.get("Type"))
        .and_then(|v| v.as_str())
        .unwrap_or("password")
        .to_string();
    let u = User {
        name: name.clone(),
        arn: arn(ctx, "user", &name),
        status: "active".to_string(),
        access_string: access,
        minimum_engine_version: "7.1".to_string(),
        authentication_mode: auth_type,
    };
    let result = json!({ "User": user_to_value(&u) });
    state.users.insert(name, u);
    Ok(result)
}

pub fn describe_users(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input.get("UserName").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .users
        .iter()
        .filter(|e| match name_filter {
            Some(n) => e.value().name == n,
            None => true,
        })
        .map(|e| user_to_value(e.value()))
        .collect();
    Ok(json!({ "Users": items }))
}

pub fn delete_user(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "UserName")?;
    let (_, u) = state.users.remove(name).ok_or_else(|| {
        AwsError::not_found("UserNotFoundFault", format!("User {name} not found"))
    })?;
    Ok(json!({ "User": user_to_value(&u) }))
}

pub fn update_user(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "UserName")?;
    let mut u = state.users.get_mut(name).ok_or_else(|| {
        AwsError::not_found("UserNotFoundFault", format!("User {name} not found"))
    })?;
    if let Some(a) = input.get("AccessString").and_then(|v| v.as_str()) {
        u.access_string = a.to_string();
    }
    Ok(json!({ "User": user_to_value(&u) }))
}

pub fn create_acl(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ACLName")?.to_string();
    if state.acls.contains_key(&name) {
        return Err(AwsError::conflict(
            "ACLAlreadyExistsFault",
            format!("ACL {name} already exists"),
        ));
    }
    let user_names: Vec<String> = input
        .get("UserNames")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let a = Acl {
        name: name.clone(),
        arn: arn(ctx, "acl", &name),
        status: "active".to_string(),
        user_names,
        minimum_engine_version: "7.1".to_string(),
    };
    let result = json!({ "ACL": acl_to_value(&a) });
    state.acls.insert(name, a);
    Ok(result)
}

pub fn describe_acls(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input.get("ACLName").and_then(|v| v.as_str());
    let items: Vec<Value> = state
        .acls
        .iter()
        .filter(|e| match name_filter {
            Some(n) => e.value().name == n,
            None => true,
        })
        .map(|e| acl_to_value(e.value()))
        .collect();
    Ok(json!({ "ACLs": items }))
}

pub fn delete_acl(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ACLName")?;
    let (_, a) = state
        .acls
        .remove(name)
        .ok_or_else(|| AwsError::not_found("ACLNotFoundFault", format!("ACL {name} not found")))?;
    Ok(json!({ "ACL": acl_to_value(&a) }))
}

pub fn create_subnet_group(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SubnetGroupName")?.to_string();
    let g = SubnetGroup {
        name: name.clone(),
        arn: arn(ctx, "subnetgroup", &name),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        vpc_id: "vpc-default".to_string(),
        subnet_ids: input
            .get("SubnetIds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let result = json!({ "SubnetGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Description": g.description,
        "VpcId": g.vpc_id,
        "Subnets": g.subnet_ids.iter().map(|id| json!({ "Identifier": id })).collect::<Vec<_>>(),
    }});
    state.subnet_groups.insert(name, g);
    Ok(result)
}

pub fn describe_subnet_groups(
    state: &MemoryDbState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .subnet_groups
        .iter()
        .map(|e| {
            let g = e.value();
            json!({
                "Name": g.name,
                "ARN": g.arn,
                "Description": g.description,
                "VpcId": g.vpc_id,
                "Subnets": g.subnet_ids.iter().map(|id| json!({ "Identifier": id })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "SubnetGroups": items }))
}

pub fn create_parameter_group(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ParameterGroupName")?.to_string();
    let g = ParameterGroup {
        name: name.clone(),
        arn: arn(ctx, "parametergroup", &name),
        family: require_str(input, "Family")?.to_string(),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let result = json!({ "ParameterGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Family": g.family,
        "Description": g.description,
    }});
    state.parameter_groups.insert(name, g);
    Ok(result)
}

pub fn describe_parameter_groups(
    state: &MemoryDbState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .parameter_groups
        .iter()
        .map(|e| {
            let g = e.value();
            json!({
                "Name": g.name,
                "ARN": g.arn,
                "Family": g.family,
                "Description": g.description,
            })
        })
        .collect();
    Ok(json!({ "ParameterGroups": items }))
}

pub fn create_snapshot(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SnapshotName")?.to_string();
    let cluster = require_str(input, "ClusterName")?.to_string();
    let s = Snapshot {
        name: name.clone(),
        arn: arn(ctx, "snapshot", &name),
        status: "available".to_string(),
        source: "manual".to_string(),
        kms_key_id: input
            .get("KmsKeyId")
            .and_then(|v| v.as_str())
            .map(String::from),
        cluster_name: cluster,
    };
    let result = json!({ "Snapshot": {
        "Name": s.name,
        "ARN": s.arn,
        "Status": s.status,
        "Source": s.source,
        "KmsKeyId": s.kms_key_id,
        "ClusterConfiguration": { "Name": s.cluster_name },
    }});
    state.snapshots.insert(name, s);
    Ok(result)
}

pub fn describe_snapshots(
    state: &MemoryDbState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .snapshots
        .iter()
        .map(|e| {
            let s = e.value();
            json!({
                "Name": s.name,
                "ARN": s.arn,
                "Status": s.status,
                "Source": s.source,
                "KmsKeyId": s.kms_key_id,
                "ClusterConfiguration": { "Name": s.cluster_name },
            })
        })
        .collect();
    Ok(json!({ "Snapshots": items }))
}

pub fn delete_snapshot(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SnapshotName")?;
    state.snapshots.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "SnapshotNotFoundFault",
            format!("Snapshot {name} not found"),
        )
    })?;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("memorydb", "us-east-1")
    }

    #[test]
    fn create_cluster_rejects_num_shards_out_of_range() {
        let state = MemoryDbState::default();
        for bad in [0i64, 501, -1] {
            let err = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c{bad}"),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "NumShards": bad,
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "InvalidParameterValueException", "input {bad}");
        }
    }

    #[test]
    fn create_cluster_rejects_num_replicas_out_of_range() {
        let state = MemoryDbState::default();
        for bad in [-1i64, 6, 100] {
            let err = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c{bad}"),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "NumReplicasPerShard": bad,
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "InvalidParameterValueException", "input {bad}");
        }
    }

    #[test]
    fn create_cluster_rejects_data_tiering_without_r6gd_node_type() {
        let state = MemoryDbState::default();
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-dt-bad",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "DataTiering": true,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
    }

    #[test]
    fn create_cluster_accepts_data_tiering_with_r6gd_node_type() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-dt-ok",
                "NodeType": "db.r6gd.xlarge",
                "ACLName": "open-access",
                "DataTiering": true,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["DataTiering"], "true");
    }

    #[test]
    fn create_cluster_defaults_data_tiering_off() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-dt-default",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["DataTiering"], "false");
    }

    #[test]
    fn create_cluster_rejects_malformed_maintenance_window() {
        let state = MemoryDbState::default();
        for bad in [
            "sun:23:00",
            "sun:23:00-sun:23:00",
            "fun:23:00-mon:01:30",
            "sun:24:00-mon:01:30",
            "sun:23:60-mon:01:30",
            "sun-mon",
            "23:00-01:30",
        ] {
            let err = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c-mw-{}", bad.len()),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "MaintenanceWindow": bad,
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "InvalidParameterValueException", "input {bad}");
        }
    }

    #[test]
    fn create_cluster_rejects_malformed_snapshot_window() {
        let state = MemoryDbState::default();
        for bad in [
            "03:00",
            "03:00-03:00",
            "24:00-04:00",
            "03:60-04:00",
            "3:00-4:00",
            "0300-0400",
        ] {
            let err = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c-sw-{}", bad.len()),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "SnapshotWindow": bad,
                }),
                &ctx(),
            )
            .unwrap_err();
            assert_eq!(err.code, "InvalidParameterValueException", "input {bad}");
        }
    }

    #[test]
    fn create_cluster_resolves_patch_version_for_each_engine() {
        let state = MemoryDbState::default();
        for (engine, patch) in [("7.1", "7.1.0"), ("7.0", "7.0.7"), ("6.2", "6.2.6")] {
            let resp = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c-engine-{engine}"),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "EngineVersion": engine,
                }),
                &ctx(),
            )
            .unwrap();
            assert_eq!(resp["Cluster"]["EngineVersion"], engine);
            assert_eq!(resp["Cluster"]["EnginePatchVersion"], patch);
        }
    }

    #[test]
    fn update_cluster_refreshes_patch_version_on_engine_bump() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-bump",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "EngineVersion": "6.2",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = update_cluster(
            &state,
            &json!({
                "ClusterName": "c-bump",
                "EngineVersion": "7.0",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["EngineVersion"], "7.0");
        assert_eq!(resp["Cluster"]["EnginePatchVersion"], "7.0.7");
    }

    #[test]
    fn create_cluster_defaults_network_fields_to_ipv4() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-net-default",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["NetworkType"], "ipv4");
        assert_eq!(resp["Cluster"]["IpDiscovery"], "ipv4");
    }

    #[test]
    fn create_cluster_accepts_dual_stack_ipv6() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-dual-stack",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NetworkType": "DUAL_STACK",
                "IpDiscovery": "IPV6",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["NetworkType"], "dual_stack");
        assert_eq!(resp["Cluster"]["IpDiscovery"], "ipv6");
    }

    #[test]
    fn create_cluster_rejects_unknown_network_type() {
        let state = MemoryDbState::default();
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-net-bad",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NetworkType": "ipv5",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_cluster_rejects_ipv6_discovery_on_ipv4_network() {
        let state = MemoryDbState::default();
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-ip-mismatch",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NetworkType": "ipv4",
                "IpDiscovery": "ipv6",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
    }

    #[test]
    fn create_cluster_accepts_well_formed_windows() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-windows-ok",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "MaintenanceWindow": "SUN:23:00-MON:01:30",
                "SnapshotWindow": "03:00-04:00",
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn create_cluster_accepts_documented_bounds() {
        let state = MemoryDbState::default();
        for (i, (shards, replicas)) in [(1i64, 0i64), (500, 5), (250, 3)].iter().enumerate() {
            create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c{i}"),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "NumShards": shards,
                    "NumReplicasPerShard": replicas,
                }),
                &ctx(),
            )
            .unwrap();
        }
    }
}
