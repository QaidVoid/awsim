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
/// release, keyed by (engine, version). Mirrors
/// `aws memorydb describe-engine-versions` output for the supported
/// control-plane engine identifiers. Redis 6.2/7.0/7.1 plus the
/// Valkey-fork 7.2 (memorydb_valkey7) and 8.0 (memorydb_valkey8)
/// engines.
const ENGINE_PATCH_VERSIONS: &[(&str, &str, &str)] = &[
    ("redis", "7.1", "7.1.0"),
    ("redis", "7.0", "7.0.7"),
    ("redis", "6.2", "6.2.6"),
    ("valkey", "8.0", "8.0.0"),
    ("valkey", "7.2", "7.2.4"),
];

fn engine_patch_version_for(engine: &str, engine_version: &str) -> Option<&'static str> {
    ENGINE_PATCH_VERSIONS
        .iter()
        .find(|(e, v, _)| *e == engine && *v == engine_version)
        .map(|(_, _, p)| *p)
}

/// Builds the per-cluster `Shards` payload. Each shard owns
/// `1 + NumReplicasPerShard` nodes; the first is the PRIMARY, the
/// rest are REPLICAs. Nodes are spread across `us-east-1a/b/c` in
/// round-robin to mirror AWS multi-AZ placement.
fn build_shards(c: &Cluster) -> Vec<Value> {
    let azs = ["us-east-1a", "us-east-1b", "us-east-1c"];
    let port = c
        .cluster_endpoint
        .get("Port")
        .and_then(Value::as_u64)
        .unwrap_or(6379);
    (1..=c.number_of_shards)
        .map(|shard_idx| {
            let shard_name = format!("{:04}", shard_idx);
            let total_nodes = 1 + c.num_replicas_per_shard;
            let nodes: Vec<Value> = (1..=total_nodes)
                .map(|node_idx| {
                    let role = if node_idx == 1 { "primary" } else { "replica" };
                    let az = azs[((shard_idx + node_idx) as usize) % azs.len()];
                    let node_name = format!("{}-{shard_name}-{:03}", c.name, node_idx);
                    json!({
                        "Name": node_name,
                        "Status": "available",
                        "AvailabilityZone": az,
                        "CreateTime": 0,
                        "RoleInShard": role,
                        "Endpoint": {
                            "Address": format!("{node_name}.{}.memorydb.amazonaws.com", c.name),
                            "Port": port,
                        },
                    })
                })
                .collect();
            json!({
                "Name": shard_name,
                "Status": "available",
                "Slots": "0-16383",
                "NumberOfNodes": total_nodes,
                "Nodes": nodes,
            })
        })
        .collect()
}

fn cluster_to_value(c: &Cluster) -> Value {
    json!({
        "Name": c.name,
        "ARN": c.arn,
        "Status": c.status,
        "NodeType": c.node_type,
        "Engine": c.engine,
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
        "SnapshotArns": c.snapshot_arns,
        "SnapshotName": c.snapshot_name,
        "MultiRegionClusterName": c.multi_region_cluster_name,
        "PendingUpdates": {
            "Resharding": Value::Null,
            "ACLs": Value::Null,
            "ServiceUpdates": [],
            "LogDeliveryConfigurations": [],
        },
        "Shards": build_shards(c),
    })
}

fn acls_referencing(state: &MemoryDbState, user_name: &str) -> Vec<String> {
    let mut acls: Vec<String> = state
        .acls
        .iter()
        .filter(|e| e.value().user_names.iter().any(|u| u == user_name))
        .map(|e| e.value().name.clone())
        .collect();
    acls.sort();
    acls
}

fn user_to_value(state: &MemoryDbState, u: &User) -> Value {
    let acl_names = acls_referencing(state, &u.name);
    json!({
        "Name": u.name,
        "ARN": u.arn,
        "Status": u.status,
        "AccessString": u.access_string,
        "MinimumEngineVersion": u.minimum_engine_version,
        "UserGroupCount": acl_names.len() as u32,
        "Authentication": {
            "Type": u.authentication_mode,
            "PasswordCount": u.password_count,
        },
        "ACLNames": acl_names,
    })
}

/// Collapses runs of ASCII whitespace into single spaces and trims
/// the result. Mirrors AWS MemoryDB's normalisation of the opaque
/// AccessString before persisting it; clients should not observe
/// raw whitespace artefacts.
fn normalise_access_string(s: &str) -> String {
    s.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses + validates a MemoryDB `AuthenticationMode` block. Returns
/// the normalised type (`password` | `iam` | `no-password-required`)
/// and the supplied `PasswordCount`. Rejects unknown types, rejects
/// `Passwords` on `iam` / `no-password-required`, and requires at
/// least one password on `password`.
fn parse_authentication_mode(input: &Value) -> Result<(String, u32), AwsError> {
    let mode = match input.get("AuthenticationMode") {
        Some(m) => m,
        None => return Ok(("password".to_string(), 0)),
    };
    let auth_type = mode
        .get("Type")
        .and_then(Value::as_str)
        .unwrap_or("password")
        .to_string();
    if !["password", "iam", "no-password-required"].contains(&auth_type.as_str()) {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!(
                "AuthenticationMode.Type `{auth_type}` must be one of password, iam, no-password-required."
            ),
        ));
    }
    let passwords = mode
        .get("Passwords")
        .and_then(Value::as_array)
        .map(|a| a.len() as u32)
        .unwrap_or(0);
    match auth_type.as_str() {
        "iam" | "no-password-required" if passwords > 0 => Err(AwsError::bad_request(
            "InvalidParameterCombinationException",
            format!("AuthenticationMode.Passwords not allowed when Type=`{auth_type}`."),
        )),
        "password" if passwords == 0 => Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "AuthenticationMode.Passwords is required when Type=`password`.".to_string(),
        )),
        _ => Ok((auth_type, passwords)),
    }
}

fn acl_to_value(a: &Acl) -> Value {
    json!({
        "Name": a.name,
        "ARN": a.arn,
        "Status": a.status,
        "UserNames": a.user_names,
        "MinimumEngineVersion": a.minimum_engine_version,
        "PendingChanges": {
            "UserNamesToAdd": [],
            "UserNamesToRemove": [],
        },
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
    let snapshot_seed = input
        .get("SnapshotName")
        .and_then(Value::as_str)
        .and_then(|sn| state.snapshots.get(sn).map(|s| s.cluster_config.clone()));
    let node_type = match input.get("NodeType").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None => snapshot_seed
            .as_ref()
            .and_then(|c| c.get("NodeType"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterValueException",
                    "NodeType is required when SnapshotName cannot supply one.",
                )
            })?
            .to_string(),
    };
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
    let engine = match input.get("Engine").and_then(Value::as_str) {
        Some(e) => {
            let lower = e.to_ascii_lowercase();
            if !matches!(lower.as_str(), "redis" | "valkey") {
                return Err(AwsError::bad_request(
                    "InvalidParameterValueException",
                    format!("Engine `{e}` must be one of redis, valkey."),
                ));
            }
            lower
        }
        None => "redis".to_string(),
    };
    let engine_version_input = input
        .get("EngineVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(if engine == "valkey" { "7.2" } else { "7.1" });
    let engine_patch =
        engine_patch_version_for(&engine, engine_version_input).ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterCombinationException",
                format!(
                    "Engine `{engine}` does not support EngineVersion `{engine_version_input}`.",
                ),
            )
        })?;
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
        engine,
        engine_version: engine_version_input.to_string(),
        engine_patch_version: engine_patch.to_string(),
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
        num_replicas_per_shard: input
            .get("NumReplicasPerShard")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0),
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
        sns_topic_status: if input.get("SnsTopicArn").and_then(Value::as_str).is_some() {
            "active".to_string()
        } else {
            "inactive".to_string()
        },
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        data_tiering,
        network_type,
        ip_discovery,
        snapshot_arns: input
            .get("SnapshotArns")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        snapshot_name: input
            .get("SnapshotName")
            .and_then(|v| v.as_str())
            .map(String::from),
        multi_region_cluster_name: input
            .get("MultiRegionClusterName")
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
    // AWS defaults ShowShardDetails to false; the heavy per-node
    // shard payload is only emitted when the caller opts in.
    let show_shards = input
        .get("ShowShardDetails")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_results = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        100,
        100,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut filtered: Vec<Cluster> = state
        .clusters
        .iter()
        .filter(|e| name_filter.is_none_or(|n| e.value().name == n))
        .map(|e| e.value().clone())
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    let page = awsim_core::paginate(filtered, max_results, starting_token, |c| c.name.clone())?;
    let items: Vec<Value> = page
        .items
        .iter()
        .map(|c| {
            let mut v = cluster_to_value(c);
            if !show_shards && let Some(obj) = v.as_object_mut() {
                obj.remove("Shards");
            }
            v
        })
        .collect();
    let mut body = json!({ "Clusters": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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
        let patch = engine_patch_version_for(&c.engine, ev).ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterCombinationException",
                format!(
                    "Engine `{}` does not support EngineVersion `{ev}`.",
                    c.engine
                ),
            )
        })?;
        c.engine_version = ev.to_string();
        c.engine_patch_version = patch.to_string();
    }
    if let Some(d) = input.get("Description").and_then(|v| v.as_str()) {
        c.description = Some(d.to_string());
    }
    if let Some(mw) = input.get("MaintenanceWindow").and_then(Value::as_str) {
        validate_maintenance_window(mw)?;
        c.maintenance_window = mw.to_string();
    }
    if let Some(sw) = input.get("SnapshotWindow").and_then(Value::as_str) {
        validate_snapshot_window(sw)?;
        c.snapshot_window = sw.to_string();
    }
    if let Some(n) = input.get("SnapshotRetentionLimit").and_then(Value::as_u64) {
        c.snapshot_retention_limit = n as u32;
    }
    // AWS treats `SnsTopicArn: ""` as "clear the topic" (status flips
    // to inactive); a non-empty ARN sets it active.
    if let Some(topic) = input.get("SnsTopicArn").and_then(Value::as_str) {
        if topic.is_empty() {
            c.sns_topic_arn = None;
            c.sns_topic_status = "inactive".to_string();
        } else {
            c.sns_topic_arn = Some(topic.to_string());
            c.sns_topic_status = "active".to_string();
        }
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
    let access = normalise_access_string(require_str(input, "AccessString")?);
    if access.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "AccessString must contain at least one non-whitespace token.".to_string(),
        ));
    }
    let (auth_type, password_count) = parse_authentication_mode(input)?;
    let u = User {
        name: name.clone(),
        arn: arn(ctx, "user", &name),
        status: "active".to_string(),
        access_string: access,
        minimum_engine_version: "7.1".to_string(),
        authentication_mode: auth_type,
        password_count,
    };
    let result = json!({ "User": user_to_value(state, &u) });
    state.users.insert(name, u);
    Ok(result)
}

pub fn describe_users(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input.get("UserName").and_then(|v| v.as_str());
    let max_results = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        100,
        100,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut filtered: Vec<User> = state
        .users
        .iter()
        .filter(|e| name_filter.is_none_or(|n| e.value().name == n))
        .map(|e| e.value().clone())
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    let page = awsim_core::paginate(filtered, max_results, starting_token, |u| u.name.clone())?;
    let items: Vec<Value> = page.items.iter().map(|u| user_to_value(state, u)).collect();
    let mut body = json!({ "Users": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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
    Ok(json!({ "User": user_to_value(state, &u) }))
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
        let normalised = normalise_access_string(a);
        if normalised.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameterValueException",
                "AccessString must contain at least one non-whitespace token.".to_string(),
            ));
        }
        u.access_string = normalised;
    }
    Ok(json!({ "User": user_to_value(state, &u) }))
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
    let max_results = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        100,
        100,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut filtered: Vec<Acl> = state
        .acls
        .iter()
        .filter(|e| name_filter.is_none_or(|n| e.value().name == n))
        .map(|e| e.value().clone())
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    let page = awsim_core::paginate(filtered, max_results, starting_token, |a| a.name.clone())?;
    let items: Vec<Value> = page.items.iter().map(acl_to_value).collect();
    let mut body = json!({ "ACLs": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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
    if state.subnet_groups.contains_key(&name) {
        return Err(AwsError::conflict(
            "SubnetGroupAlreadyExistsFault",
            format!("Subnet group {name} already exists"),
        ));
    }
    let subnet_ids: Vec<String> = input
        .get("SubnetIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if subnet_ids.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "SubnetIds must contain at least one subnet.",
        ));
    }
    let g = SubnetGroup {
        name: name.clone(),
        arn: arn(ctx, "subnetgroup", &name),
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        vpc_id: "vpc-default".to_string(),
        subnet_ids,
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

pub fn update_subnet_group(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SubnetGroupName")?;
    let mut g = state.subnet_groups.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "SubnetGroupNotFoundFault",
            format!("Subnet group {name} not found"),
        )
    })?;
    if let Some(d) = input.get("Description").and_then(|v| v.as_str()) {
        g.description = Some(d.to_string());
    }
    if let Some(arr) = input.get("SubnetIds").and_then(|v| v.as_array()) {
        let subnet_ids: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if subnet_ids.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameterValueException",
                "SubnetIds must contain at least one subnet.",
            ));
        }
        g.subnet_ids = subnet_ids;
    }
    Ok(json!({ "SubnetGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Description": g.description,
        "VpcId": g.vpc_id,
        "Subnets": g.subnet_ids.iter().map(|id| json!({ "Identifier": id })).collect::<Vec<_>>(),
    }}))
}

pub fn delete_subnet_group(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SubnetGroupName")?;
    if name == "default" {
        return Err(AwsError::bad_request(
            "InvalidSubnetGroupStateFault",
            "The default subnet group cannot be deleted.",
        ));
    }
    let in_use = state
        .clusters
        .iter()
        .any(|c| c.value().subnet_group_name == name);
    if in_use {
        return Err(AwsError::conflict(
            "SubnetGroupInUseFault",
            format!("Subnet group {name} is still attached to one or more clusters."),
        ));
    }
    let (_, g) = state.subnet_groups.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "SubnetGroupNotFoundFault",
            format!("Subnet group {name} not found"),
        )
    })?;
    Ok(json!({ "SubnetGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Description": g.description,
        "VpcId": g.vpc_id,
        "Subnets": g.subnet_ids.iter().map(|id| json!({ "Identifier": id })).collect::<Vec<_>>(),
    }}))
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
    if state.parameter_groups.contains_key(&name) {
        return Err(AwsError::conflict(
            "ParameterGroupAlreadyExistsFault",
            format!("Parameter group {name} already exists"),
        ));
    }
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

pub fn delete_parameter_group(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ParameterGroupName")?;
    if name.starts_with("default.") {
        return Err(AwsError::bad_request(
            "InvalidParameterGroupStateFault",
            format!("The default parameter group {name} cannot be deleted."),
        ));
    }
    let in_use = state
        .clusters
        .iter()
        .any(|c| c.value().parameter_group_name == name);
    if in_use {
        return Err(AwsError::conflict(
            "InvalidParameterGroupStateFault",
            format!("Parameter group {name} is still attached to one or more clusters."),
        ));
    }
    let (_, g) = state.parameter_groups.remove(name).ok_or_else(|| {
        AwsError::not_found(
            "ParameterGroupNotFoundFault",
            format!("Parameter group {name} not found"),
        )
    })?;
    Ok(json!({ "ParameterGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Family": g.family,
        "Description": g.description,
    }}))
}

pub fn reset_parameter_group(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ParameterGroupName")?;
    let g = state.parameter_groups.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ParameterGroupNotFoundFault",
            format!("Parameter group {name} not found"),
        )
    })?;
    let all_parameters = input
        .get("AllParameters")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let parameter_names: Vec<String> = input
        .get("ParameterNames")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    // AWS rejects redundant inputs (both AllParameters=true and a
    // ParameterNames list) as well as the empty case where neither
    // field tells the API which parameters to reset.
    if all_parameters && !parameter_names.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterCombinationException",
            "ParameterNames must be empty when AllParameters=true.",
        ));
    }
    if !all_parameters && parameter_names.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "ParameterNames must not be empty when AllParameters=false.",
        ));
    }
    Ok(json!({ "ParameterGroup": {
        "Name": g.name,
        "ARN": g.arn,
        "Family": g.family,
        "Description": g.description,
    }}))
}

/// Seeded MemoryDB service-update fixtures. Each entry mirrors the
/// shape AWS returns from `DescribeServiceUpdates`. The fixture is
/// intentionally small; callers exercising the filter/pagination
/// surface get deterministic output without an external feed.
const SERVICE_UPDATE_FIXTURES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "memorydb-2024-redis-7-1-patch-001",
        "redis",
        "7.1",
        "security-update",
        "available",
    ),
    (
        "memorydb-2024-valkey-7-2-patch-001",
        "valkey",
        "7.2",
        "security-update",
        "available",
    ),
    (
        "memorydb-2024-redis-6-2-patch-005",
        "redis",
        "6.2",
        "security-update",
        "available",
    ),
];

fn service_update_to_value(
    name: &str,
    engine: &str,
    engine_version: &str,
    update_type: &str,
    status: &str,
) -> Value {
    json!({
        "ServiceUpdateName": name,
        "Engine": engine,
        "EngineVersion": engine_version,
        "ServiceUpdateType": update_type,
        "Status": status,
        "Severity": "important",
        "Description": format!("Routine {engine} {engine_version} patch."),
    })
}

/// Applies a service-update to one or more clusters. Mirrors AWS by
/// splitting the request into ProcessedClusters (cluster exists) and
/// UnprocessedClusters (with an error reason). The chip stops short of
/// mutating PendingUpdates; the surface is enough for clients that
/// orchestrate fleet-wide patching.
pub fn batch_update_cluster(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let service_update_name = input
        .get("ServiceUpdate")
        .and_then(|v| v.get("ServiceUpdateNameToApply"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterValueException",
                "ServiceUpdate.ServiceUpdateNameToApply is required.",
            )
        })?;
    let known_update = SERVICE_UPDATE_FIXTURES
        .iter()
        .any(|(n, _, _, _, _)| *n == service_update_name);
    if !known_update {
        return Err(AwsError::not_found(
            "ServiceUpdateNotFoundFault",
            format!("Service update {service_update_name} not found"),
        ));
    }
    let cluster_names: Vec<&str> = input
        .get("ClusterNames")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if cluster_names.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "ClusterNames must contain at least one cluster.",
        ));
    }
    let mut processed = Vec::new();
    let mut unprocessed = Vec::new();
    for name in cluster_names {
        if let Some(c) = state.clusters.get(name) {
            processed.push(cluster_to_value(c.value()));
        } else {
            unprocessed.push(json!({
                "ClusterName": name,
                "ErrorType": "ClusterNotFoundFault",
                "ErrorMessage": format!("Cluster {name} not found"),
            }));
        }
    }
    Ok(json!({
        "ProcessedClusters": processed,
        "UnprocessedClusters": unprocessed,
    }))
}

pub fn describe_service_updates(
    _state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name_filter = input.get("ServiceUpdateName").and_then(Value::as_str);
    let status_filter: Option<Vec<&str>> = input
        .get("Status")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect());
    if let Some(statuses) = status_filter.as_ref() {
        for s in statuses {
            if !matches!(*s, "available" | "in-progress" | "complete" | "scheduled") {
                return Err(AwsError::bad_request(
                    "InvalidParameterValueException",
                    format!(
                        "Status `{s}` must be one of available, in-progress, complete, scheduled.",
                    ),
                ));
            }
        }
    }
    let max_results = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        100,
        100,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut updates: Vec<(String, Value)> = SERVICE_UPDATE_FIXTURES
        .iter()
        .filter(|(n, _, _, _, s)| {
            name_filter.is_none_or(|wanted| *n == wanted)
                && status_filter
                    .as_ref()
                    .is_none_or(|set| set.iter().any(|w| w == s))
        })
        .map(|(n, e, v, t, s)| (n.to_string(), service_update_to_value(n, e, v, t, s)))
        .collect();
    updates.sort_by(|a, b| a.0.cmp(&b.0));
    let page = awsim_core::paginate(updates, max_results, starting_token, |(k, _)| k.clone())?;
    let items: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();
    let mut body = json!({ "ServiceUpdates": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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

/// Builds the `ClusterConfiguration` block returned with snapshot
/// responses. Mirrors AWS by capturing every topology field a restore
/// would need so the snapshot remains useful after the source cluster
/// is deleted.
fn snapshot_cluster_configuration(c: &Cluster) -> Value {
    let port = c
        .cluster_endpoint
        .get("Port")
        .and_then(Value::as_u64)
        .unwrap_or(6379);
    json!({
        "Name": c.name,
        "Description": c.description,
        "NodeType": c.node_type,
        "EngineVersion": c.engine_version,
        "MaintenanceWindow": c.maintenance_window,
        "Port": port,
        "ParameterGroupName": c.parameter_group_name,
        "SubnetGroupName": c.subnet_group_name,
        "VpcId": "vpc-default",
        "SnapshotRetentionLimit": c.snapshot_retention_limit,
        "SnapshotWindow": c.snapshot_window,
        "NumShards": c.number_of_shards,
        "Shards": build_shards(c),
    })
}

fn snapshot_to_value(s: &Snapshot) -> Value {
    let config = if s.cluster_config.is_null() {
        json!({ "Name": s.cluster_name })
    } else {
        s.cluster_config.clone()
    };
    json!({
        "Name": s.name,
        "ARN": s.arn,
        "Status": s.status,
        "Source": s.source,
        "KmsKeyId": s.kms_key_id,
        "ClusterConfiguration": config,
    })
}

pub fn create_snapshot(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "SnapshotName")?.to_string();
    let cluster_name = require_str(input, "ClusterName")?.to_string();
    let cluster_config = state
        .clusters
        .get(&cluster_name)
        .map(|c| snapshot_cluster_configuration(c.value()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ClusterNotFoundFault",
                format!("Cluster {cluster_name} not found"),
            )
        })?;
    let s = Snapshot {
        name: name.clone(),
        arn: arn(ctx, "snapshot", &name),
        status: "available".to_string(),
        source: "manual".to_string(),
        kms_key_id: input
            .get("KmsKeyId")
            .and_then(|v| v.as_str())
            .map(String::from),
        cluster_name,
        cluster_config,
    };
    let result = json!({ "Snapshot": snapshot_to_value(&s) });
    state.snapshots.insert(name, s);
    Ok(result)
}

pub fn copy_snapshot(
    state: &MemoryDbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_name = require_str(input, "SourceSnapshotName")?.to_string();
    let target_name = require_str(input, "TargetSnapshotName")?.to_string();
    if input.get("TargetBucket").is_some() {
        // AWS routes TargetBucket copies through the S3 export path
        // (see spec 0008). Until S3 integration lands, surface the
        // unsupported-mode rejection rather than silently dropping.
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "CopySnapshot with TargetBucket is not supported by this emulator.",
        ));
    }
    if state.snapshots.contains_key(&target_name) {
        return Err(AwsError::conflict(
            "SnapshotAlreadyExistsFault",
            format!("Snapshot {target_name} already exists"),
        ));
    }
    let source = state.snapshots.get(&source_name).ok_or_else(|| {
        AwsError::not_found(
            "SnapshotNotFoundFault",
            format!("Snapshot {source_name} not found"),
        )
    })?;
    let kms_override = input
        .get("KmsKeyId")
        .and_then(Value::as_str)
        .map(String::from);
    let copy = Snapshot {
        name: target_name.clone(),
        arn: arn(ctx, "snapshot", &target_name),
        status: "available".to_string(),
        source: "manual".to_string(),
        kms_key_id: kms_override.or_else(|| source.kms_key_id.clone()),
        cluster_name: source.cluster_name.clone(),
        cluster_config: source.cluster_config.clone(),
    };
    drop(source);
    let result = json!({ "Snapshot": snapshot_to_value(&copy) });
    state.snapshots.insert(target_name, copy);
    Ok(result)
}

pub fn describe_snapshots(
    state: &MemoryDbState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let snapshot_name = input.get("SnapshotName").and_then(Value::as_str);
    let cluster_name = input.get("ClusterName").and_then(Value::as_str);
    let source = input.get("Source").and_then(Value::as_str);
    if let Some(s) = source
        && !matches!(s, "manual" | "automated")
    {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            format!("Source `{s}` must be one of manual, automated."),
        ));
    }
    let max_results = awsim_core::clamp_max_results_strict(
        input.get("MaxResults").and_then(Value::as_i64),
        50,
        50,
    )?;
    let starting_token = input.get("NextToken").and_then(Value::as_str);
    let mut filtered: Vec<Snapshot> = state
        .snapshots
        .iter()
        .filter(|e| {
            let s = e.value();
            snapshot_name.is_none_or(|n| s.name == n)
                && cluster_name.is_none_or(|n| s.cluster_name == n)
                && source.is_none_or(|src| s.source == src)
        })
        .map(|e| e.value().clone())
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    let page = awsim_core::paginate(filtered, max_results, starting_token, |s| s.name.clone())?;
    let items: Vec<Value> = page.items.iter().map(snapshot_to_value).collect();
    let mut body = json!({ "Snapshots": items });
    if let Some(token) = page.next_token {
        body["NextToken"] = json!(token);
    }
    Ok(body)
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
    fn update_subnet_group_replaces_description_and_subnets() {
        let state = MemoryDbState::default();
        create_subnet_group(
            &state,
            &json!({
                "SubnetGroupName": "sg-up",
                "Description": "old",
                "SubnetIds": ["subnet-a"],
            }),
            &ctx(),
        )
        .unwrap();
        let resp = update_subnet_group(
            &state,
            &json!({
                "SubnetGroupName": "sg-up",
                "Description": "new",
                "SubnetIds": ["subnet-b", "subnet-c"],
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["SubnetGroup"]["Description"], "new");
        let subnets: Vec<&str> = resp["SubnetGroup"]["Subnets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Identifier"].as_str().unwrap())
            .collect();
        assert_eq!(subnets, vec!["subnet-b", "subnet-c"]);
    }

    #[test]
    fn update_subnet_group_rejects_empty_subnet_ids() {
        let state = MemoryDbState::default();
        create_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "sg-empty", "SubnetIds": ["subnet-x"] }),
            &ctx(),
        )
        .unwrap();
        let err = update_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "sg-empty", "SubnetIds": [] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn update_subnet_group_returns_not_found_for_unknown_name() {
        let state = MemoryDbState::default();
        let err = update_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "ghost", "Description": "x" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "SubnetGroupNotFoundFault");
    }

    #[test]
    fn create_subnet_group_rejects_duplicate_name() {
        let state = MemoryDbState::default();
        create_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "dup", "SubnetIds": ["subnet-a"] }),
            &ctx(),
        )
        .unwrap();
        let err = create_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "dup", "SubnetIds": ["subnet-b"] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "SubnetGroupAlreadyExistsFault");
    }

    #[test]
    fn create_subnet_group_rejects_empty_subnet_ids() {
        let state = MemoryDbState::default();
        let err = create_subnet_group(&state, &json!({ "SubnetGroupName": "empty" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_parameter_group_rejects_duplicate_name() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-dup",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        let err = create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-dup",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ParameterGroupAlreadyExistsFault");
    }

    #[test]
    fn delete_subnet_group_rejects_default_name() {
        let state = MemoryDbState::default();
        let err = delete_subnet_group(&state, &json!({ "SubnetGroupName": "default" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "InvalidSubnetGroupStateFault");
    }

    #[test]
    fn delete_subnet_group_rejects_when_cluster_attached() {
        let state = MemoryDbState::default();
        create_subnet_group(
            &state,
            &json!({
                "SubnetGroupName": "in-use",
                "SubnetIds": ["subnet-aaa", "subnet-bbb"],
            }),
            &ctx(),
        )
        .unwrap();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "uses-subnet",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "SubnetGroupName": "in-use",
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_subnet_group(&state, &json!({ "SubnetGroupName": "in-use" }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "SubnetGroupInUseFault");
    }

    #[test]
    fn delete_subnet_group_removes_unused_group() {
        let state = MemoryDbState::default();
        create_subnet_group(
            &state,
            &json!({ "SubnetGroupName": "free", "SubnetIds": ["subnet-x"] }),
            &ctx(),
        )
        .unwrap();
        delete_subnet_group(&state, &json!({ "SubnetGroupName": "free" }), &ctx()).unwrap();
        assert!(!state.subnet_groups.contains_key("free"));
    }

    #[test]
    fn delete_parameter_group_rejects_default_prefix() {
        let state = MemoryDbState::default();
        let err = delete_parameter_group(
            &state,
            &json!({ "ParameterGroupName": "default.memorydb-redis7" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterGroupStateFault");
    }

    #[test]
    fn delete_parameter_group_rejects_when_cluster_attached() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-in-use",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "uses-pg",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "ParameterGroupName": "pg-in-use",
            }),
            &ctx(),
        )
        .unwrap();
        let err = delete_parameter_group(
            &state,
            &json!({ "ParameterGroupName": "pg-in-use" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterGroupStateFault");
    }

    #[test]
    fn delete_parameter_group_removes_unused_group() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-free",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        delete_parameter_group(&state, &json!({ "ParameterGroupName": "pg-free" }), &ctx())
            .unwrap();
        assert!(!state.parameter_groups.contains_key("pg-free"));
    }

    #[test]
    fn batch_update_cluster_splits_processed_and_unprocessed() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "live",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = batch_update_cluster(
            &state,
            &json!({
                "ServiceUpdate": { "ServiceUpdateNameToApply": "memorydb-2024-redis-7-1-patch-001" },
                "ClusterNames": ["live", "ghost"],
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["ProcessedClusters"].as_array().unwrap().len(), 1);
        assert_eq!(resp["ProcessedClusters"][0]["Name"], "live");
        let unprocessed = resp["UnprocessedClusters"].as_array().unwrap();
        assert_eq!(unprocessed.len(), 1);
        assert_eq!(unprocessed[0]["ClusterName"], "ghost");
        assert_eq!(unprocessed[0]["ErrorType"], "ClusterNotFoundFault");
    }

    #[test]
    fn batch_update_cluster_rejects_unknown_service_update_name() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "live",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        let err = batch_update_cluster(
            &state,
            &json!({
                "ServiceUpdate": { "ServiceUpdateNameToApply": "memorydb-fictitious" },
                "ClusterNames": ["live"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ServiceUpdateNotFoundFault");
    }

    #[test]
    fn batch_update_cluster_rejects_empty_cluster_names() {
        let state = MemoryDbState::default();
        let err = batch_update_cluster(
            &state,
            &json!({
                "ServiceUpdate": { "ServiceUpdateNameToApply": "memorydb-2024-redis-7-1-patch-001" },
                "ClusterNames": [],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn describe_service_updates_returns_all_seeded_entries_by_default() {
        let state = MemoryDbState::default();
        let resp = describe_service_updates(&state, &json!({}), &ctx()).unwrap();
        let count = resp["ServiceUpdates"].as_array().unwrap().len();
        assert_eq!(count, SERVICE_UPDATE_FIXTURES.len());
    }

    #[test]
    fn describe_service_updates_filters_by_service_update_name() {
        let state = MemoryDbState::default();
        let resp = describe_service_updates(
            &state,
            &json!({ "ServiceUpdateName": "memorydb-2024-valkey-7-2-patch-001" }),
            &ctx(),
        )
        .unwrap();
        let updates = resp["ServiceUpdates"].as_array().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["Engine"], "valkey");
    }

    #[test]
    fn describe_service_updates_rejects_unknown_status() {
        let state = MemoryDbState::default();
        let err = describe_service_updates(&state, &json!({ "Status": ["pending"] }), &ctx())
            .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn describe_service_updates_paginates_with_max_results() {
        let state = MemoryDbState::default();
        let first = describe_service_updates(&state, &json!({ "MaxResults": 1 }), &ctx()).unwrap();
        assert_eq!(first["ServiceUpdates"].as_array().unwrap().len(), 1);
        let token = first["NextToken"].as_str().unwrap().to_string();
        let second = describe_service_updates(
            &state,
            &json!({ "MaxResults": 1, "NextToken": token }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(second["ServiceUpdates"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn reset_parameter_group_accepts_all_parameters_flag() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-all",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        reset_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-all",
                "AllParameters": true,
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn reset_parameter_group_accepts_specific_parameter_names() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-some",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        reset_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-some",
                "ParameterNames": ["maxmemory-policy"],
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn reset_parameter_group_rejects_all_and_names_combination() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-both",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        let err = reset_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-both",
                "AllParameters": true,
                "ParameterNames": ["maxmemory-policy"],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
    }

    #[test]
    fn reset_parameter_group_rejects_empty_names_when_all_false() {
        let state = MemoryDbState::default();
        create_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "pg-reset-empty",
                "Family": "memorydb_redis7",
            }),
            &ctx(),
        )
        .unwrap();
        let err = reset_parameter_group(
            &state,
            &json!({ "ParameterGroupName": "pg-reset-empty" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn reset_parameter_group_returns_not_found_for_unknown_name() {
        let state = MemoryDbState::default();
        let err = reset_parameter_group(
            &state,
            &json!({
                "ParameterGroupName": "missing",
                "AllParameters": true,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ParameterGroupNotFoundFault");
    }

    #[test]
    fn describe_clusters_paginates_with_max_results_and_next_token() {
        let state = MemoryDbState::default();
        for name in ["alpha", "bravo", "charlie", "delta", "echo"] {
            create_cluster(
                &state,
                &json!({
                    "ClusterName": name,
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                }),
                &ctx(),
            )
            .unwrap();
        }
        let first = describe_clusters(&state, &json!({ "MaxResults": 2 }), &ctx()).unwrap();
        let names: Vec<String> = first["Clusters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "bravo"]);
        let token = first["NextToken"].as_str().unwrap().to_string();
        let second = describe_clusters(
            &state,
            &json!({ "MaxResults": 2, "NextToken": token }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<String> = second["Clusters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["charlie", "delta"]);
    }

    #[test]
    fn describe_clusters_elides_shards_by_default() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-show",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NumShards": 2,
            }),
            &ctx(),
        )
        .unwrap();
        let default_resp = describe_clusters(&state, &json!({}), &ctx()).unwrap();
        let cluster = &default_resp["Clusters"][0];
        assert!(
            cluster.get("Shards").is_none(),
            "Shards should be absent by default: {cluster}"
        );
        let detailed =
            describe_clusters(&state, &json!({ "ShowShardDetails": true }), &ctx()).unwrap();
        let shards = detailed["Clusters"][0]["Shards"].as_array().unwrap();
        assert_eq!(shards.len(), 2);
    }

    #[test]
    fn update_cluster_persists_validated_window_changes() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-windows-update",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = update_cluster(
            &state,
            &json!({
                "ClusterName": "c-windows-update",
                "MaintenanceWindow": "tue:02:00-tue:04:00",
                "SnapshotWindow": "05:00-06:00",
                "SnapshotRetentionLimit": 14,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["MaintenanceWindow"], "tue:02:00-tue:04:00");
        assert_eq!(resp["Cluster"]["SnapshotWindow"], "05:00-06:00");
        assert_eq!(resp["Cluster"]["SnapshotRetentionLimit"], 14);
    }

    #[test]
    fn update_cluster_rejects_malformed_maintenance_window() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-wupd-bad",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        let err = update_cluster(
            &state,
            &json!({
                "ClusterName": "c-wupd-bad",
                "MaintenanceWindow": "garbage",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_cluster_defaults_auto_minor_version_upgrade_true() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-amvu-default",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["AutoMinorVersionUpgrade"], true);
    }

    #[test]
    fn create_cluster_honors_auto_minor_version_upgrade_false() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-amvu-off",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "AutoMinorVersionUpgrade": false,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["AutoMinorVersionUpgrade"], false);
    }

    #[test]
    fn create_cluster_emits_empty_pending_updates_block() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-pending",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        let pending = &resp["Cluster"]["PendingUpdates"];
        assert!(pending["Resharding"].is_null());
        assert!(pending["ACLs"].is_null());
        assert!(pending["ServiceUpdates"].as_array().unwrap().is_empty());
        assert!(
            pending["LogDeliveryConfigurations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn create_cluster_defaults_engine_to_redis() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-engine-default",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["Engine"], "redis");
    }

    #[test]
    fn create_cluster_accepts_valkey_engine() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-valkey",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "Engine": "VALKEY",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["Engine"], "valkey");
    }

    #[test]
    fn create_cluster_rejects_unknown_engine() {
        let state = MemoryDbState::default();
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-engine-bad",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "Engine": "memcached",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_cluster_resolves_patch_version_for_each_engine() {
        let state = MemoryDbState::default();
        for (engine, version, patch) in [
            ("redis", "7.1", "7.1.0"),
            ("redis", "7.0", "7.0.7"),
            ("redis", "6.2", "6.2.6"),
            ("valkey", "7.2", "7.2.4"),
            ("valkey", "8.0", "8.0.0"),
        ] {
            let resp = create_cluster(
                &state,
                &json!({
                    "ClusterName": format!("c-{engine}-{version}"),
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                    "Engine": engine,
                    "EngineVersion": version,
                }),
                &ctx(),
            )
            .unwrap();
            assert_eq!(resp["Cluster"]["Engine"], engine);
            assert_eq!(resp["Cluster"]["EngineVersion"], version);
            assert_eq!(resp["Cluster"]["EnginePatchVersion"], patch);
        }
    }

    #[test]
    fn create_cluster_rejects_engine_version_coupling_mismatch() {
        let state = MemoryDbState::default();
        // redis doesn't run valkey 8.0.
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-mismatch",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "Engine": "redis",
                "EngineVersion": "8.0",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
        // valkey doesn't run redis 6.2.
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-mismatch2",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "Engine": "valkey",
                "EngineVersion": "6.2",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
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

    #[test]
    fn create_cluster_sns_topic_status_inactive_without_arn() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-sns-none",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["SnsTopicStatus"], "inactive");
        assert!(resp["Cluster"]["SnsTopicArn"].is_null());
    }

    #[test]
    fn create_cluster_sns_topic_status_active_with_arn() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "c-sns-set",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "SnsTopicArn": "arn:aws:sns:us-east-1:111111111111:alerts",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["SnsTopicStatus"], "active");
    }

    #[test]
    fn update_cluster_clears_sns_topic_when_empty_arn_supplied() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "c-sns-clear",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "SnsTopicArn": "arn:aws:sns:us-east-1:111111111111:alerts",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = update_cluster(
            &state,
            &json!({
                "ClusterName": "c-sns-clear",
                "SnsTopicArn": "",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["SnsTopicStatus"], "inactive");
        assert!(resp["Cluster"]["SnsTopicArn"].is_null());
    }

    #[test]
    fn create_cluster_builds_shards_with_primary_and_replicas() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "topo",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NumShards": 2,
                "NumReplicasPerShard": 2,
            }),
            &ctx(),
        )
        .unwrap();
        let shards = resp["Cluster"]["Shards"].as_array().unwrap();
        assert_eq!(shards.len(), 2);
        for shard in shards {
            let nodes = shard["Nodes"].as_array().unwrap();
            assert_eq!(nodes.len(), 3);
            assert_eq!(nodes[0]["RoleInShard"], "primary");
            assert_eq!(nodes[1]["RoleInShard"], "replica");
            assert_eq!(nodes[2]["RoleInShard"], "replica");
            assert_eq!(shard["NumberOfNodes"], 3);
        }
        assert_eq!(shards[0]["Name"], "0001");
        assert_eq!(shards[1]["Name"], "0002");
        let first_node = &shards[0]["Nodes"][0];
        assert_eq!(first_node["Name"], "topo-0001-001");
        assert_eq!(first_node["Endpoint"]["Port"], 6379);
    }

    #[test]
    fn create_cluster_builds_single_node_shard_when_no_replicas() {
        let state = MemoryDbState::default();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "solo",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
                "NumShards": 1,
            }),
            &ctx(),
        )
        .unwrap();
        let nodes = resp["Cluster"]["Shards"][0]["Nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["RoleInShard"], "primary");
    }

    #[test]
    fn create_cluster_seeds_node_type_from_snapshot() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "donor",
                "NodeType": "db.r6g.2xlarge",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        create_snapshot(
            &state,
            &json!({ "SnapshotName": "donor-snap", "ClusterName": "donor" }),
            &ctx(),
        )
        .unwrap();
        let resp = create_cluster(
            &state,
            &json!({
                "ClusterName": "restored",
                "ACLName": "open-access",
                "SnapshotName": "donor-snap",
                "SnapshotArns": ["arn:aws:s3:::backup/x"],
                "MultiRegionClusterName": "mr-1",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Cluster"]["NodeType"], "db.r6g.2xlarge");
        assert_eq!(resp["Cluster"]["SnapshotName"], "donor-snap");
        assert_eq!(resp["Cluster"]["MultiRegionClusterName"], "mr-1");
        let arns: Vec<&str> = resp["Cluster"]["SnapshotArns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(arns, vec!["arn:aws:s3:::backup/x"]);
    }

    #[test]
    fn create_cluster_requires_node_type_without_snapshot_seed() {
        let state = MemoryDbState::default();
        let err = create_cluster(
            &state,
            &json!({
                "ClusterName": "no-node-type",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn copy_snapshot_clones_source_into_new_name() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "src-cl",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        create_snapshot(
            &state,
            &json!({ "SnapshotName": "orig", "ClusterName": "src-cl" }),
            &ctx(),
        )
        .unwrap();
        let resp = copy_snapshot(
            &state,
            &json!({ "SourceSnapshotName": "orig", "TargetSnapshotName": "clone" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Snapshot"]["Name"], "clone");
        assert_eq!(resp["Snapshot"]["ClusterConfiguration"]["Name"], "src-cl");
        assert!(state.snapshots.contains_key("clone"));
    }

    #[test]
    fn copy_snapshot_rejects_target_bucket_path() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "src-cl2",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        create_snapshot(
            &state,
            &json!({ "SnapshotName": "orig2", "ClusterName": "src-cl2" }),
            &ctx(),
        )
        .unwrap();
        let err = copy_snapshot(
            &state,
            &json!({
                "SourceSnapshotName": "orig2",
                "TargetSnapshotName": "exported",
                "TargetBucket": "my-bucket",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn copy_snapshot_returns_not_found_for_missing_source() {
        let state = MemoryDbState::default();
        let err = copy_snapshot(
            &state,
            &json!({ "SourceSnapshotName": "ghost", "TargetSnapshotName": "next" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "SnapshotNotFoundFault");
    }

    #[test]
    fn copy_snapshot_rejects_target_already_exists() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "src-cl3",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        for name in ["a", "b"] {
            create_snapshot(
                &state,
                &json!({ "SnapshotName": name, "ClusterName": "src-cl3" }),
                &ctx(),
            )
            .unwrap();
        }
        let err = copy_snapshot(
            &state,
            &json!({ "SourceSnapshotName": "a", "TargetSnapshotName": "b" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "SnapshotAlreadyExistsFault");
    }

    #[test]
    fn describe_snapshots_filters_by_cluster_and_paginates() {
        let state = MemoryDbState::default();
        for cluster in ["cl-a", "cl-b"] {
            create_cluster(
                &state,
                &json!({
                    "ClusterName": cluster,
                    "NodeType": "db.r6g.large",
                    "ACLName": "open-access",
                }),
                &ctx(),
            )
            .unwrap();
        }
        for (snap, cluster) in [
            ("alpha", "cl-a"),
            ("bravo", "cl-a"),
            ("charlie", "cl-a"),
            ("delta", "cl-b"),
        ] {
            create_snapshot(
                &state,
                &json!({ "SnapshotName": snap, "ClusterName": cluster }),
                &ctx(),
            )
            .unwrap();
        }
        let first = describe_snapshots(
            &state,
            &json!({ "ClusterName": "cl-a", "MaxResults": 2 }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<String> = first["Snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "bravo"]);
        let token = first["NextToken"].as_str().unwrap().to_string();
        let second = describe_snapshots(
            &state,
            &json!({ "ClusterName": "cl-a", "MaxResults": 2, "NextToken": token }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<String> = second["Snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["charlie"]);
    }

    #[test]
    fn describe_snapshots_filters_by_source_value() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "src-cluster",
                "NodeType": "db.r6g.large",
                "ACLName": "open-access",
            }),
            &ctx(),
        )
        .unwrap();
        create_snapshot(
            &state,
            &json!({ "SnapshotName": "manual-1", "ClusterName": "src-cluster" }),
            &ctx(),
        )
        .unwrap();
        let manual = describe_snapshots(&state, &json!({ "Source": "manual" }), &ctx()).unwrap();
        assert_eq!(manual["Snapshots"].as_array().unwrap().len(), 1);
        let automated =
            describe_snapshots(&state, &json!({ "Source": "automated" }), &ctx()).unwrap();
        assert!(automated["Snapshots"].as_array().unwrap().is_empty());
    }

    #[test]
    fn describe_snapshots_rejects_unknown_source() {
        let state = MemoryDbState::default();
        let err =
            describe_snapshots(&state, &json!({ "Source": "scheduled" }), &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_snapshot_emits_full_cluster_configuration() {
        let state = MemoryDbState::default();
        create_cluster(
            &state,
            &json!({
                "ClusterName": "src-cluster",
                "NodeType": "db.r6g.xlarge",
                "ACLName": "open-access",
                "EngineVersion": "7.0",
                "MaintenanceWindow": "sun:23:00-mon:01:30",
                "SnapshotWindow": "03:00-04:00",
                "SnapshotRetentionLimit": 7,
                "Description": "primary cluster",
                "NumShards": 3,
            }),
            &ctx(),
        )
        .unwrap();
        let resp = create_snapshot(
            &state,
            &json!({
                "SnapshotName": "snap-1",
                "ClusterName": "src-cluster",
            }),
            &ctx(),
        )
        .unwrap();
        let cfg = &resp["Snapshot"]["ClusterConfiguration"];
        assert_eq!(cfg["Name"], "src-cluster");
        assert_eq!(cfg["NodeType"], "db.r6g.xlarge");
        assert_eq!(cfg["EngineVersion"], "7.0");
        assert_eq!(cfg["MaintenanceWindow"], "sun:23:00-mon:01:30");
        assert_eq!(cfg["SnapshotWindow"], "03:00-04:00");
        assert_eq!(cfg["SnapshotRetentionLimit"], 7);
        assert_eq!(cfg["NumShards"], 3);
        assert_eq!(cfg["Port"], 6379);
        assert_eq!(cfg["Description"], "primary cluster");
    }

    #[test]
    fn create_snapshot_rejects_missing_cluster() {
        let state = MemoryDbState::default();
        let err = create_snapshot(
            &state,
            &json!({
                "SnapshotName": "snap-orphan",
                "ClusterName": "does-not-exist",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ClusterNotFoundFault");
    }

    #[test]
    fn create_user_defaults_password_count_to_zero_without_passwords() {
        let state = MemoryDbState::default();
        let resp = create_user(
            &state,
            &json!({
                "UserName": "u-default",
                "AccessString": "on ~* +@all",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["User"]["Authentication"]["Type"], "password");
        assert_eq!(resp["User"]["Authentication"]["PasswordCount"], 0);
    }

    #[test]
    fn create_user_accepts_password_mode_with_passwords() {
        let state = MemoryDbState::default();
        let resp = create_user(
            &state,
            &json!({
                "UserName": "u-pwd",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": {
                    "Type": "password",
                    "Passwords": ["hunter2hunter2"],
                },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["User"]["Authentication"]["Type"], "password");
        assert_eq!(resp["User"]["Authentication"]["PasswordCount"], 1);
    }

    #[test]
    fn create_user_accepts_iam_authentication() {
        let state = MemoryDbState::default();
        let resp = create_user(
            &state,
            &json!({
                "UserName": "u-iam",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["User"]["Authentication"]["Type"], "iam");
        assert_eq!(resp["User"]["Authentication"]["PasswordCount"], 0);
    }

    #[test]
    fn create_user_rejects_passwords_on_iam_type() {
        let state = MemoryDbState::default();
        let err = create_user(
            &state,
            &json!({
                "UserName": "u-iam-bad",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": {
                    "Type": "iam",
                    "Passwords": ["leaked-secret"],
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterCombinationException");
    }

    #[test]
    fn create_user_rejects_unknown_authentication_type() {
        let state = MemoryDbState::default();
        let err = create_user(
            &state,
            &json!({
                "UserName": "u-bad-type",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "saml" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn create_user_normalises_access_string_whitespace() {
        let state = MemoryDbState::default();
        let resp = create_user(
            &state,
            &json!({
                "UserName": "u-ws",
                "AccessString": "  on   ~*    +@all  ",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["User"]["AccessString"], "on ~* +@all");
    }

    #[test]
    fn create_user_rejects_blank_access_string() {
        let state = MemoryDbState::default();
        let err = create_user(
            &state,
            &json!({
                "UserName": "u-blank",
                "AccessString": "   \t  ",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }

    #[test]
    fn update_user_normalises_access_string() {
        let state = MemoryDbState::default();
        create_user(
            &state,
            &json!({
                "UserName": "u-up",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = update_user(
            &state,
            &json!({
                "UserName": "u-up",
                "AccessString": "off    ~keys:*   +get",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["User"]["AccessString"], "off ~keys:* +get");
    }

    #[test]
    fn describe_users_populates_user_group_count_and_acl_names() {
        let state = MemoryDbState::default();
        create_user(
            &state,
            &json!({
                "UserName": "alice",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "iam" },
            }),
            &ctx(),
        )
        .unwrap();
        create_acl(
            &state,
            &json!({ "ACLName": "team-a", "UserNames": ["alice"] }),
            &ctx(),
        )
        .unwrap();
        create_acl(
            &state,
            &json!({ "ACLName": "team-b", "UserNames": ["alice", "bob"] }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_users(&state, &json!({ "UserName": "alice" }), &ctx()).unwrap();
        let user = &resp["Users"][0];
        assert_eq!(user["UserGroupCount"], 2);
        let names: Vec<&str> = user["ACLNames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["team-a", "team-b"]);
    }

    #[test]
    fn describe_acls_emits_empty_pending_changes_block() {
        let state = MemoryDbState::default();
        create_acl(
            &state,
            &json!({ "ACLName": "team-empty", "UserNames": [] }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_acls(&state, &json!({ "ACLName": "team-empty" }), &ctx()).unwrap();
        let pending = &resp["ACLs"][0]["PendingChanges"];
        assert!(pending["UserNamesToAdd"].as_array().unwrap().is_empty());
        assert!(pending["UserNamesToRemove"].as_array().unwrap().is_empty());
    }

    #[test]
    fn create_user_requires_passwords_when_type_password_and_block_present() {
        let state = MemoryDbState::default();
        let err = create_user(
            &state,
            &json!({
                "UserName": "u-pwd-missing",
                "AccessString": "on ~* +@all",
                "AuthenticationMode": { "Type": "password" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValueException");
    }
}
