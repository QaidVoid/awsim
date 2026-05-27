use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Addon, EksState, now_secs};

/// AWS-accepted values for `resolveConflicts`. PRESERVE keeps the
/// existing in-cluster configuration on conflict; OVERWRITE replaces
/// it; NONE fails the call when any field already exists with a
/// different value. We don't model the in-cluster state, so the
/// validation surface here is shape-only — the persisted value drives
/// the merge strategy on later UpdateAddon calls.
const RESOLVE_CONFLICTS_VALUES: &[&str] = &["NONE", "OVERWRITE", "PRESERVE"];

fn validate_resolve_conflicts(value: &str) -> Result<(), AwsError> {
    if RESOLVE_CONFLICTS_VALUES.contains(&value) {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "resolveConflicts `{value}` is not one of {}.",
                RESOLVE_CONFLICTS_VALUES.join(", "),
            ),
        ))
    }
}

fn validate_configuration_values(value: &str) -> Result<(), AwsError> {
    if value.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "configurationValues must not be an empty string.",
        ));
    }
    // AWS accepts either JSON or YAML; for the simulator's purposes we
    // only require that the payload is non-empty and that, when it
    // looks like JSON (starts with `{` or `[`), it parses. A plain YAML
    // string is left to the caller's discretion.
    let trimmed = value.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<Value>(value).is_err()
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "configurationValues looks like JSON but failed to parse.",
        ));
    }
    Ok(())
}

fn addon_arn(region: &str, account: &str, cluster: &str, addon: &str) -> String {
    format!("arn:aws:eks:{region}:{account}:addon/{cluster}/{addon}")
}

fn serialize_addon(a: &Addon) -> Value {
    let mut obj = json!({
        "addonName": a.addon_name,
        "clusterName": a.cluster_name,
        "addonArn": a.addon_arn,
        "addonVersion": a.addon_version,
        "status": a.status,
        "createdAt": a.created_at,
        "modifiedAt": a.modified_at,
        "tags": a.tags,
        "resolveConflicts": a.resolve_conflicts,
    });
    if let Some(ref role) = a.service_account_role_arn {
        obj["serviceAccountRoleArn"] = Value::String(role.clone());
    }
    if let Some(ref cfg) = a.configuration_values {
        obj["configurationValues"] = Value::String(cfg.clone());
    }
    obj
}

fn require_cluster<'a>(
    state: &'a EksState,
    cluster_name: &str,
) -> Result<dashmap::mapref::one::Ref<'a, String, crate::state::Cluster>, AwsError> {
    state.clusters.get(cluster_name).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Cluster {cluster_name} not found."),
        )
    })
}

pub fn create_addon(
    state: &EksState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required.")
    })?;
    let addon_name = input["addonName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "addonName is required.")
    })?;
    require_cluster(state, cluster_name)?;

    let key = (cluster_name.to_string(), addon_name.to_string());
    if state.addons.contains_key(&key) {
        return Err(AwsError::conflict(
            "ResourceInUseException",
            format!("Addon {addon_name} already exists on cluster {cluster_name}."),
        ));
    }

    let resolve_conflicts = input["resolveConflicts"]
        .as_str()
        .unwrap_or("NONE")
        .to_string();
    validate_resolve_conflicts(&resolve_conflicts)?;

    if let Some(cfg) = input["configurationValues"].as_str() {
        validate_configuration_values(cfg)?;
    }

    let now = now_secs();
    let addon = Addon {
        cluster_name: cluster_name.to_string(),
        addon_name: addon_name.to_string(),
        addon_arn: addon_arn(&ctx.region, &ctx.account_id, cluster_name, addon_name),
        addon_version: input["addonVersion"]
            .as_str()
            .unwrap_or("v1.0.0-eksbuild.1")
            .to_string(),
        status: "ACTIVE".to_string(),
        service_account_role_arn: input["serviceAccountRoleArn"].as_str().map(str::to_string),
        resolve_conflicts,
        configuration_values: input["configurationValues"].as_str().map(str::to_string),
        tags: input["tags"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: now,
        modified_at: now,
    };
    let serialized = serialize_addon(&addon);
    state.addons.insert(key, addon);
    Ok(json!({ "addon": serialized }))
}

pub fn describe_addon(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required.")
    })?;
    let addon_name = input["addonName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "addonName is required.")
    })?;
    let key = (cluster_name.to_string(), addon_name.to_string());
    let addon = state.addons.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Addon {addon_name} not found on cluster {cluster_name}."),
        )
    })?;
    Ok(json!({ "addon": serialize_addon(&addon) }))
}

pub fn list_addons(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required.")
    })?;
    require_cluster(state, cluster_name)?;
    let mut addons: Vec<String> = state
        .addons
        .iter()
        .filter(|e| e.key().0 == cluster_name)
        .map(|e| e.key().1.clone())
        .collect();
    addons.sort();
    Ok(json!({ "addons": addons }))
}

pub fn update_addon(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required.")
    })?;
    let addon_name = input["addonName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "addonName is required.")
    })?;
    let key = (cluster_name.to_string(), addon_name.to_string());
    let mut addon = state.addons.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Addon {addon_name} not found on cluster {cluster_name}."),
        )
    })?;

    let resolve_conflicts = input["resolveConflicts"]
        .as_str()
        .unwrap_or(addon.resolve_conflicts.as_str())
        .to_string();
    validate_resolve_conflicts(&resolve_conflicts)?;

    let new_cfg = input.get("configurationValues").and_then(Value::as_str);
    if let Some(cfg) = new_cfg {
        validate_configuration_values(cfg)?;
    }

    // PRESERVE keeps the existing configurationValues even when the
    // caller supplies a new one — AWS interprets PRESERVE as "don't
    // clobber what's already in the cluster". OVERWRITE and NONE both
    // replace it on a successful call.
    if let Some(cfg) = new_cfg
        && resolve_conflicts != "PRESERVE"
    {
        addon.configuration_values = Some(cfg.to_string());
    }

    if let Some(v) = input["addonVersion"].as_str() {
        addon.addon_version = v.to_string();
    }
    if let Some(role) = input.get("serviceAccountRoleArn").and_then(Value::as_str) {
        addon.service_account_role_arn = Some(role.to_string());
    }
    addon.resolve_conflicts = resolve_conflicts;
    addon.modified_at = now_secs();

    Ok(json!({ "update": {
        "id": format!("{cluster_name}-{addon_name}-update"),
        "status": "Successful",
        "type": "AddonUpdate",
    } }))
}

pub fn delete_addon(
    state: &EksState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_name = input["clusterName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "clusterName is required.")
    })?;
    let addon_name = input["addonName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "addonName is required.")
    })?;
    let key = (cluster_name.to_string(), addon_name.to_string());
    let (_, addon) = state.addons.remove(&key).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Addon {addon_name} not found on cluster {cluster_name}."),
        )
    })?;
    Ok(json!({ "addon": serialize_addon(&addon) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn state_with_cluster() -> EksState {
        let state = EksState::default();
        state.clusters.insert(
            "demo".into(),
            crate::state::Cluster {
                name: "demo".into(),
                arn: "arn:aws:eks:us-east-1:000000000000:cluster/demo".into(),
                version: "1.29".into(),
                endpoint: "https://demo.eks.us-east-1.amazonaws.com".into(),
                role_arn: "arn:aws:iam::000000000000:role/eks".into(),
                resources_vpc_config: Value::Null,
                kubernetes_network_config: Value::Null,
                logging: Value::Null,
                identity: Value::Null,
                status: "ACTIVE".into(),
                certificate_authority: Value::Null,
                platform_version: "eks.1".into(),
                tags: HashMap::new(),
                created_at: 0,
                encryption_config: vec![],
            },
        );
        state
    }

    fn ctx() -> RequestContext {
        RequestContext::new("eks", "us-east-1")
    }

    #[test]
    fn create_rejects_invalid_resolve_conflicts() {
        let state = state_with_cluster();
        let err = create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "resolveConflicts": "MAYBE",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_persists_configuration_and_resolve_conflicts() {
        let state = state_with_cluster();
        let out = create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "resolveConflicts": "OVERWRITE",
                "configurationValues": "{\"env\":\"prod\"}",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(out["addon"]["resolveConflicts"], "OVERWRITE");
        assert_eq!(out["addon"]["configurationValues"], "{\"env\":\"prod\"}");
    }

    #[test]
    fn create_rejects_malformed_json_configuration() {
        let state = state_with_cluster();
        let err = create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "configurationValues": "{not-json",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_accepts_plain_yaml_configuration() {
        let state = state_with_cluster();
        create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "configurationValues": "env: prod\nlevel: info\n",
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn update_with_preserve_keeps_existing_configuration() {
        let state = state_with_cluster();
        create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "configurationValues": "{\"keep\":true}",
            }),
            &ctx(),
        )
        .unwrap();
        update_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "resolveConflicts": "PRESERVE",
                "configurationValues": "{\"keep\":false}",
            }),
            &ctx(),
        )
        .unwrap();
        let out = describe_addon(
            &state,
            &json!({ "clusterName": "demo", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(out["addon"]["configurationValues"], "{\"keep\":true}");
    }

    #[test]
    fn update_with_overwrite_replaces_configuration() {
        let state = state_with_cluster();
        create_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "configurationValues": "{\"keep\":true}",
            }),
            &ctx(),
        )
        .unwrap();
        update_addon(
            &state,
            &json!({
                "clusterName": "demo",
                "addonName": "vpc-cni",
                "resolveConflicts": "OVERWRITE",
                "configurationValues": "{\"keep\":false}",
            }),
            &ctx(),
        )
        .unwrap();
        let out = describe_addon(
            &state,
            &json!({ "clusterName": "demo", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(out["addon"]["configurationValues"], "{\"keep\":false}");
    }

    #[test]
    fn list_addons_returns_sorted_names() {
        let state = state_with_cluster();
        for name in ["vpc-cni", "coredns", "kube-proxy"] {
            create_addon(
                &state,
                &json!({ "clusterName": "demo", "addonName": name }),
                &ctx(),
            )
            .unwrap();
        }
        let out = list_addons(&state, &json!({ "clusterName": "demo" }), &ctx()).unwrap();
        let names = out["addons"].as_array().unwrap();
        assert_eq!(
            names
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["coredns", "kube-proxy", "vpc-cni"]
        );
    }

    #[test]
    fn delete_removes_addon() {
        let state = state_with_cluster();
        create_addon(
            &state,
            &json!({ "clusterName": "demo", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap();
        delete_addon(
            &state,
            &json!({ "clusterName": "demo", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap();
        let err = describe_addon(
            &state,
            &json!({ "clusterName": "demo", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn create_on_unknown_cluster_returns_not_found() {
        let state = EksState::default();
        let err = create_addon(
            &state,
            &json!({ "clusterName": "ghost", "addonName": "vpc-cni" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }
}
