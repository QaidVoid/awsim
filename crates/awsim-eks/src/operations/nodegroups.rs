use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use awsim_core::lifecycle::LifecycleSm;
use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::{EksState, Nodegroup, NodegroupState, now_secs};

/// Wall-clock a nodegroup spends in `CREATING` before promoting to
/// `ACTIVE`. Collapsed to zero by `AWSIM_LIFECYCLE_FAST`.
const NODEGROUP_CREATE_DELAY: Duration = Duration::from_secs(3);

pub fn create_nodegroup(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required")
    })?;
    let name = input["nodegroupName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "nodegroupName is required")
    })?;

    // AWS requires at least one subnet; the nodegroup launches its
    // ASG into those subnets, so an empty list has no semantic.
    let subnets: Vec<String> = input["subnets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if subnets.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "subnets is required and must contain at least one subnet id.",
        ));
    }

    // AWS allows diskSize between 1 and 16_384 GiB. Anything else is
    // a documented ValidationException.
    let disk_size = input["diskSize"].as_u64().unwrap_or(20);
    if !(1..=16_384).contains(&disk_size) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("diskSize must be between 1 and 16384 GiB (got {disk_size})."),
        ));
    }

    // Taints persist verbatim, but `effect` must be a Kubernetes-defined
    // value. AWS rejects unknown effects with InvalidParameterException
    // before persisting the nodegroup.
    let taints: Vec<Value> = input["taints"].as_array().cloned().unwrap_or_default();
    for t in &taints {
        let effect = t.get("effect").and_then(Value::as_str).unwrap_or("");
        if !matches!(effect, "NO_SCHEDULE" | "NO_EXECUTE" | "PREFER_NO_SCHEDULE") {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!(
                    "taints.effect `{effect}` must be one of: \
                     NO_SCHEDULE, NO_EXECUTE, PREFER_NO_SCHEDULE."
                ),
            ));
        }
    }

    let labels: HashMap<String, String> = input["labels"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // remoteAccess.sourceSecurityGroups requires ec2SshKey per AWS docs;
    // accepting the SGs without an SSH key would silently drop them at
    // launch.
    let remote_access = match input.get("remoteAccess") {
        Some(v) if !v.is_null() => {
            let obj = v.as_object().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "remoteAccess must be an object.",
                )
            })?;
            let has_sgs = obj
                .get("sourceSecurityGroups")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty());
            let ssh_key = obj.get("ec2SshKey").and_then(Value::as_str);
            if has_sgs && ssh_key.is_none_or(str::is_empty) {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    "remoteAccess.sourceSecurityGroups requires ec2SshKey.",
                ));
            }
            Some(v.clone())
        }
        _ => None,
    };

    // launchTemplate: exactly one of name/id must be present, version
    // optional. Both-or-neither matches AWS's InvalidParameterException.
    let launch_template = match input.get("launchTemplate") {
        Some(v) if !v.is_null() => {
            let obj = v.as_object().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "launchTemplate must be an object.",
                )
            })?;
            let has_name = obj
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            let has_id = obj
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            if has_name == has_id {
                return Err(AwsError::bad_request(
                    "InvalidParameterException",
                    "launchTemplate requires exactly one of name or id.",
                ));
            }
            Some(v.clone())
        }
        _ => None,
    };

    let arn = arn::build(
        ctx,
        "eks",
        format!(
            "nodegroup/{}/{}/{}",
            cluster,
            name,
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ),
    );
    let ng = Nodegroup {
        cluster_name: cluster.to_string(),
        name: name.to_string(),
        arn: arn.clone(),
        status: NodegroupState::Creating.as_wire().to_string(),
        capacity_type: input["capacityType"]
            .as_str()
            .unwrap_or("ON_DEMAND")
            .to_string(),
        scaling_config: input["scalingConfig"].clone(),
        instance_types: input["instanceTypes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["t3.medium".to_string()]),
        subnets,
        ami_type: input["amiType"]
            .as_str()
            .unwrap_or("AL2_x86_64")
            .to_string(),
        node_role: input["nodeRole"].as_str().unwrap_or("").to_string(),
        version: input["version"].as_str().unwrap_or("1.29").to_string(),
        release_version: input["releaseVersion"]
            .as_str()
            .unwrap_or("1.29.0-20240101")
            .to_string(),
        disk_size: disk_size as u32,
        tags: input["tags"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: now_secs(),
        labels,
        taints,
        remote_access,
        launch_template,
        sm: LifecycleSm::new(NodegroupState::Creating),
    };
    // Schedule the CREATING -> ACTIVE promotion; tick (or a polling
    // DescribeNodegroup) observes the deadline and flips the status.
    ng.sm.start_transition(
        NodegroupState::Creating,
        NodegroupState::Active,
        NODEGROUP_CREATE_DELAY,
    );
    let key = (cluster.to_string(), name.to_string());
    state.nodegroups.insert(key.clone(), ng);
    let ng = state.nodegroups.get(&key).expect("just inserted");
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn describe_nodegroup(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["nodegroupName"].as_str().unwrap_or("");
    let ng = state
        .nodegroups
        .get(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Nodegroup {name} not found"),
            )
        })?;
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn delete_nodegroup(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let name = input["nodegroupName"].as_str().unwrap_or("");
    let (_, ng) = state
        .nodegroups
        .remove(&(cluster.to_string(), name.to_string()))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Nodegroup {name} not found"),
            )
        })?;
    Ok(json!({ "nodegroup": serialize_nodegroup(&ng) }))
}

pub fn list_nodegroups(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster = input["clusterName"].as_str().unwrap_or("");
    let names: Vec<String> = state
        .nodegroups
        .iter()
        .filter(|e| e.key().0 == cluster)
        .map(|e| e.key().1.clone())
        .collect();
    Ok(json!({ "nodegroups": names }))
}

fn serialize_nodegroup(ng: &Nodegroup) -> Value {
    // AWS surfaces `resources.autoScalingGroups[].name` so the caller
    // can find the underlying EC2 ASG. The ASG name is derived from
    // the nodegroup ARN suffix; we synthesize a stable id from the
    // nodegroup id segment so the response shape matches without
    // standing up a real ASG.
    let asg_suffix = ng
        .arn
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&ng.name);
    let asg_name = format!("eks-{}-{}", ng.name, asg_suffix);
    // Derive the wire `status` from the live state machine so a
    // polling DescribeNodegroup observes CREATING -> ACTIVE as the
    // scheduled deadline elapses.
    let status = ng.sm.observe(SystemTime::now()).state.as_wire();
    let mut obj = json!({
        "nodegroupName": ng.name,
        "nodegroupArn": ng.arn,
        "clusterName": ng.cluster_name,
        "version": ng.version,
        "releaseVersion": ng.release_version,
        "createdAt": ng.created_at,
        "modifiedAt": ng.created_at,
        "status": status,
        "capacityType": ng.capacity_type,
        "scalingConfig": ng.scaling_config,
        "instanceTypes": ng.instance_types,
        "subnets": ng.subnets,
        "amiType": ng.ami_type,
        "nodeRole": ng.node_role,
        "diskSize": ng.disk_size,
        "tags": ng.tags,
        "labels": ng.labels,
        "taints": ng.taints,
        "resources": {
            "autoScalingGroups": [{ "name": asg_name }],
            "remoteAccessSecurityGroup": Value::Null,
        },
    });
    if let Some(ref ra) = ng.remote_access {
        obj["remoteAccess"] = ra.clone();
    }
    if let Some(ref lt) = ng.launch_template {
        obj["launchTemplate"] = lt.clone();
    }
    obj
}

#[cfg(test)]
mod nodegroup_extras_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("eks", "us-east-1")
    }

    fn base_input() -> Value {
        json!({
            "clusterName": "c1",
            "nodegroupName": "ng1",
            "subnets": ["subnet-aaa"],
            "nodeRole": "arn:aws:iam::000000000000:role/eks-ng",
        })
    }

    #[test]
    fn persists_labels_and_taints() {
        let state = EksState::default();
        let mut input = base_input();
        input["labels"] = json!({ "team": "platform" });
        input["taints"] = json!([
            { "key": "dedicated", "value": "gpu", "effect": "NO_SCHEDULE" }
        ]);
        let resp = create_nodegroup(&state, &input, &ctx()).unwrap();
        let ng = &resp["nodegroup"];
        assert_eq!(ng["labels"]["team"], "platform");
        assert_eq!(ng["taints"][0]["effect"], "NO_SCHEDULE");
    }

    #[test]
    fn rejects_taint_effect_outside_kubernetes_enum() {
        let state = EksState::default();
        let mut input = base_input();
        input["taints"] = json!([
            { "key": "k", "value": "v", "effect": "BOGUS" }
        ]);
        let err = create_nodegroup(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn persists_remote_access_with_ssh_key_and_sgs() {
        let state = EksState::default();
        let mut input = base_input();
        input["remoteAccess"] = json!({
            "ec2SshKey": "my-key",
            "sourceSecurityGroups": ["sg-aaa"],
        });
        let resp = create_nodegroup(&state, &input, &ctx()).unwrap();
        let ng = &resp["nodegroup"];
        assert_eq!(ng["remoteAccess"]["ec2SshKey"], "my-key");
    }

    #[test]
    fn persists_launch_template_with_id() {
        let state = EksState::default();
        let mut input = base_input();
        input["launchTemplate"] = json!({ "id": "lt-aaa", "version": "1" });
        let resp = create_nodegroup(&state, &input, &ctx()).unwrap();
        assert_eq!(resp["nodegroup"]["launchTemplate"]["id"], "lt-aaa");
    }

    #[test]
    fn rejects_launch_template_with_both_name_and_id() {
        let state = EksState::default();
        let mut input = base_input();
        input["launchTemplate"] = json!({ "id": "lt-aaa", "name": "foo" });
        let err = create_nodegroup(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_launch_template_with_neither_name_nor_id() {
        let state = EksState::default();
        let mut input = base_input();
        input["launchTemplate"] = json!({ "version": "1" });
        let err = create_nodegroup(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn nodegroup_resources_include_synthetic_asg_name() {
        let state = EksState::default();
        let resp = create_nodegroup(&state, &base_input(), &ctx()).unwrap();
        let asgs = resp["nodegroup"]["resources"]["autoScalingGroups"]
            .as_array()
            .expect("autoScalingGroups populated");
        assert_eq!(asgs.len(), 1);
        let name = asgs[0]["name"].as_str().unwrap();
        assert!(name.starts_with("eks-ng1-"));
    }

    #[test]
    fn rejects_remote_access_sgs_without_ssh_key() {
        let state = EksState::default();
        let mut input = base_input();
        input["remoteAccess"] = json!({
            "sourceSecurityGroups": ["sg-aaa"],
        });
        let err = create_nodegroup(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("ec2SshKey"));
    }
}
