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
    let node_type = require_str(input, "NodeType")?.to_string();
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
        engine_patch_version: "7.1.0".to_string(),
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
