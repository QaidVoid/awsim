use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{
    EcrState, PullThroughCacheRule, RegistryScanningConfiguration, ReplicationConfiguration,
};

fn now_epoch_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input[key].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", format!("{key} is required"))
    })
}

fn repo_not_found(name: &str) -> AwsError {
    AwsError::bad_request(
        "RepositoryNotFoundException",
        format!("The repository with name '{name}' does not exist in the registry"),
    )
}

pub fn put_image_tag_mutability(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_str(input, "repositoryName")?;
    let mutability = require_str(input, "imageTagMutability")?;

    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;
    repo.image_tag_mutability = mutability.to_string();

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "imageTagMutability": mutability,
    }))
}

pub fn put_image_scanning_configuration(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_str(input, "repositoryName")?;
    let scan_on_push = input["imageScanningConfiguration"]["scanOnPush"]
        .as_bool()
        .unwrap_or(false);

    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;
    repo.scan_on_push = scan_on_push;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "imageScanningConfiguration": {
            "scanOnPush": scan_on_push,
        },
    }))
}

pub fn start_lifecycle_policy_preview(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_str(input, "repositoryName")?;
    let policy = input["lifecyclePolicyText"].as_str();

    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let preview = policy
        .map(|s| s.to_string())
        .or_else(|| repo.lifecycle_policy.clone())
        .ok_or_else(|| {
            AwsError::bad_request(
                "LifecyclePolicyNotFoundException",
                format!("Lifecycle policy for repository '{repo_name}' not found"),
            )
        })?;
    repo.lifecycle_policy_preview = Some(preview.clone());

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": preview,
        "status": "COMPLETE",
    }))
}

pub fn get_lifecycle_policy_preview(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_str(input, "repositoryName")?;
    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let preview = repo
        .lifecycle_policy_preview
        .as_deref()
        .or(repo.lifecycle_policy.as_deref())
        .ok_or_else(|| {
            AwsError::bad_request(
                "LifecyclePolicyPreviewNotFoundException",
                format!("Lifecycle policy preview for repository '{repo_name}' not found"),
            )
        })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": preview,
        "status": "COMPLETE",
        "previewResults": [],
        "summary": {
            "expiringImageTotalCount": 0u32,
        },
    }))
}

pub fn get_registry_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let _ = input;
    let policy = state
        .registry_policy
        .get("default")
        .map(|p| p.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request(
                "RegistryPolicyNotFoundException",
                "Registry policy does not exist",
            )
        })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "policyText": policy,
    }))
}

pub fn put_registry_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policy = require_str(input, "policyText")?;
    state
        .registry_policy
        .insert("default".to_string(), policy.to_string());

    Ok(json!({
        "registryId": ctx.account_id,
        "policyText": policy,
    }))
}

pub fn delete_registry_policy(
    state: &EcrState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let removed = state
        .registry_policy
        .remove("default")
        .map(|(_, v)| v)
        .ok_or_else(|| {
            AwsError::bad_request(
                "RegistryPolicyNotFoundException",
                "Registry policy does not exist",
            )
        })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "policyText": removed,
    }))
}

pub fn describe_registry(
    state: &EcrState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cfg = state.replication_config.read().unwrap().clone();
    Ok(json!({
        "registryId": ctx.account_id,
        "replicationConfiguration": {
            "rules": cfg.rules,
        },
    }))
}

pub fn get_registry_scanning_configuration(
    state: &EcrState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cfg = state.registry_scanning_config.read().unwrap().clone();
    let scan_type = if cfg.scan_type.is_empty() {
        "BASIC".to_string()
    } else {
        cfg.scan_type
    };
    Ok(json!({
        "registryId": ctx.account_id,
        "scanningConfiguration": {
            "scanType": scan_type,
            "rules": cfg.rules,
        },
    }))
}

pub fn put_registry_scanning_configuration(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let scan_type = input["scanType"].as_str().unwrap_or("BASIC").to_string();
    let rules: Vec<Value> = input["rules"].as_array().cloned().unwrap_or_default();

    let new_cfg = RegistryScanningConfiguration {
        scan_type: scan_type.clone(),
        rules: rules.clone(),
    };
    *state.registry_scanning_config.write().unwrap() = new_cfg;

    Ok(json!({
        "registryScanningConfiguration": {
            "scanType": scan_type,
            "rules": rules,
        },
        "registryId": ctx.account_id,
    }))
}

pub fn put_replication_configuration(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let rules: Vec<Value> = input["replicationConfiguration"]["rules"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    *state.replication_config.write().unwrap() = ReplicationConfiguration {
        rules: rules.clone(),
    };

    Ok(json!({
        "replicationConfiguration": {
            "rules": rules,
        },
    }))
}

pub fn batch_get_repository_scanning_configuration(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = input["repositoryNames"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut configs = Vec::new();
    let mut failures = Vec::new();

    for name in names {
        match state.repositories.get(&name) {
            Some(repo) => {
                configs.push(json!({
                    "repositoryArn": repo.arn,
                    "repositoryName": name,
                    "scanOnPush": repo.scan_on_push,
                    "scanFrequency": "MANUAL",
                    "appliedScanFilters": [],
                }));
            }
            None => failures.push(json!({
                "repositoryName": name,
                "failureCode": "REPOSITORY_NOT_FOUND",
                "failureReason": "Repository not found",
            })),
        }
    }

    Ok(json!({
        "scanningConfigurations": configs,
        "failures": failures,
    }))
}

pub fn create_pull_through_cache_rule(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let prefix = require_str(input, "ecrRepositoryPrefix")?.to_string();
    let upstream = require_str(input, "upstreamRegistryUrl")?.to_string();

    if state.pull_through_cache_rules.contains_key(&prefix) {
        return Err(AwsError::bad_request(
            "PullThroughCacheRuleAlreadyExistsException",
            format!("A pull-through cache rule with prefix '{prefix}' already exists"),
        ));
    }

    let upstream_registry = input["upstreamRegistry"].as_str().map(|s| s.to_string());
    let credential_arn = input["credentialArn"].as_str().map(|s| s.to_string());
    let created_at = now_epoch_str();

    let rule = PullThroughCacheRule {
        ecr_repository_prefix: prefix.clone(),
        upstream_registry_url: upstream.clone(),
        upstream_registry: upstream_registry.clone(),
        credential_arn: credential_arn.clone(),
        created_at: created_at.clone(),
    };
    state.pull_through_cache_rules.insert(prefix.clone(), rule);

    Ok(json!({
        "ecrRepositoryPrefix": prefix,
        "upstreamRegistryUrl": upstream,
        "createdAt": created_at,
        "registryId": ctx.account_id,
        "upstreamRegistry": upstream_registry,
        "credentialArn": credential_arn,
    }))
}

pub fn delete_pull_through_cache_rule(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let prefix = require_str(input, "ecrRepositoryPrefix")?;
    let (_, rule) = state
        .pull_through_cache_rules
        .remove(prefix)
        .ok_or_else(|| {
            AwsError::bad_request(
                "PullThroughCacheRuleNotFoundException",
                format!("Pull-through cache rule with prefix '{prefix}' not found"),
            )
        })?;

    Ok(json!({
        "ecrRepositoryPrefix": rule.ecr_repository_prefix,
        "upstreamRegistryUrl": rule.upstream_registry_url,
        "createdAt": rule.created_at,
        "registryId": ctx.account_id,
        "credentialArn": rule.credential_arn,
    }))
}

pub fn describe_pull_through_cache_rules(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter: Vec<String> = input["ecrRepositoryPrefixes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let rules: Vec<Value> = state
        .pull_through_cache_rules
        .iter()
        .filter(|e| filter.is_empty() || filter.contains(&e.value().ecr_repository_prefix))
        .map(|e| {
            let r = e.value();
            json!({
                "ecrRepositoryPrefix": r.ecr_repository_prefix,
                "upstreamRegistryUrl": r.upstream_registry_url,
                "createdAt": r.created_at,
                "registryId": ctx.account_id,
                "upstreamRegistry": r.upstream_registry,
                "credentialArn": r.credential_arn,
            })
        })
        .collect();

    Ok(json!({
        "pullThroughCacheRules": rules,
    }))
}

pub fn get_account_setting(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "name")?;
    let value = state
        .account_settings
        .get(name)
        .map(|v| v.value().clone())
        .unwrap_or_else(|| "BASIC_SCAN_TYPE_VERSION".to_string());

    Ok(json!({
        "name": name,
        "value": value,
    }))
}

pub fn put_account_setting(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "name")?.to_string();
    let value = require_str(input, "value")?.to_string();
    state.account_settings.insert(name.clone(), value.clone());

    Ok(json!({
        "name": name,
        "value": value,
    }))
}
