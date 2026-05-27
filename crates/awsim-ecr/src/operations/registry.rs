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
    // Validate the policy up front — AWS rejects malformed previews
    // the same way it rejects PutLifecyclePolicy.
    crate::operations::extras::parse_lifecycle_policy(&preview)?;
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

    let policy = crate::operations::extras::parse_lifecycle_policy(preview)?;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let expired =
        crate::operations::extras::evaluate_lifecycle_policy(&policy, &repo.images, now_secs);

    let preview_results: Vec<Value> = expired
        .iter()
        .map(|(digest, priority, desc)| {
            let img = repo.images.iter().find(|i| &i.image_digest == digest);
            let tags = img
                .and_then(|i| i.image_tag.clone())
                .map(|t| vec![t])
                .unwrap_or_default();
            json!({
                "imageTags": tags,
                "imageDigest": digest,
                "imagePushedAt": img.map(|i| i.pushed_at.as_str()).unwrap_or(""),
                "action": { "type": "expire" },
                "appliedRulePriority": priority,
                "ruleDescription": desc,
            })
        })
        .collect();

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": preview,
        "status": "COMPLETE",
        "previewResults": preview_results,
        "summary": {
            "expiringImageTotalCount": expired.len() as u32,
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
    // Mirror SetRepositoryPolicy: registry-level policies must also
    // be valid JSON objects with a Statement array.
    let parsed: Value = serde_json::from_str(policy).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("policyText is not valid JSON: {e}"),
        )
    })?;
    if !parsed.is_object()
        || !parsed
            .get("Statement")
            .map(|s| s.is_array())
            .unwrap_or(false)
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "policyText must be a JSON object containing a Statement array.",
        ));
    }
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

/// AWS-documented upstream registry types for pull-through cache
/// rules. New entries arrive over time (most recent additions:
/// `ecr-public`, `github-container-registry`); keep this in lockstep
/// with the public CFN docs.
const PULL_THROUGH_REGISTRY_KINDS: &[&str] = &[
    "ecr",
    "ecr-public",
    "docker-hub",
    "quay",
    "k8s",
    "github-container-registry",
    "azure-container-registry",
    "gitlab-container-registry",
];

/// Validate the `upstreamRegistryUrl` (and optional `upstreamRegistry`
/// enum) supplied to CreatePullThroughCacheRule. AWS rejects calls
/// that point at an unknown upstream with `ValidationException`; the
/// simulator mirrors that so a typo doesn't sail through and resolve
/// to a no-op layer fetch later.
fn validate_pull_through_upstream(url: &str, kind: Option<&str>) -> Result<(), AwsError> {
    if let Some(kind) = kind
        && !PULL_THROUGH_REGISTRY_KINDS.contains(&kind)
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "upstreamRegistry `{kind}` must be one of: {}.",
                PULL_THROUGH_REGISTRY_KINDS.join(", "),
            ),
        ));
    }

    let inferred = infer_pull_through_kind(url).ok_or_else(|| {
        AwsError::bad_request(
            "ValidationException",
            format!(
                "upstreamRegistryUrl `{url}` is not a recognised public registry. \
                 Expected one of: public.ecr.aws, docker.io / registry-1.docker.io, \
                 quay.io, registry.k8s.io, ghcr.io, *.azurecr.io, registry.gitlab.com, \
                 or <account>.dkr.ecr.<region>.amazonaws.com."
            ),
        )
    })?;

    if let Some(kind) = kind
        && kind != inferred
    {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "upstreamRegistry `{kind}` does not match the upstreamRegistryUrl \
                 (inferred `{inferred}` from `{url}`)."
            ),
        ));
    }
    Ok(())
}

/// Infer the upstreamRegistry kind from the URL. Returns `None` for
/// URLs that don't match any AWS-documented upstream.
fn infer_pull_through_kind(url: &str) -> Option<&'static str> {
    let normalized = url.to_ascii_lowercase();
    let host = normalized
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = host.split('/').next().unwrap_or(host);
    let host = host.to_string();
    if host == "public.ecr.aws" {
        return Some("ecr-public");
    }
    if host == "docker.io" || host == "registry-1.docker.io" || host == "index.docker.io" {
        return Some("docker-hub");
    }
    if host == "quay.io" {
        return Some("quay");
    }
    if host == "registry.k8s.io" || host == "k8s.gcr.io" {
        return Some("k8s");
    }
    if host == "ghcr.io" {
        return Some("github-container-registry");
    }
    if host.ends_with(".azurecr.io") {
        return Some("azure-container-registry");
    }
    if host == "registry.gitlab.com" {
        return Some("gitlab-container-registry");
    }
    // Cross-account ECR: <account>.dkr.ecr.<region>.amazonaws.com
    if host.ends_with(".amazonaws.com") && host.contains(".dkr.ecr.") {
        return Some("ecr");
    }
    None
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
    validate_pull_through_upstream(&upstream, upstream_registry.as_deref())?;
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

#[cfg(test)]
mod pull_through_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("ecr", "us-east-1")
    }

    #[test]
    fn create_rejects_unknown_upstream_url() {
        let state = EcrState::default();
        let err = create_pull_through_cache_rule(
            &state,
            &json!({
                "ecrRepositoryPrefix": "mirror",
                "upstreamRegistryUrl": "https://my-mirror.example.com",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_rejects_unknown_upstream_kind() {
        let state = EcrState::default();
        let err = create_pull_through_cache_rule(
            &state,
            &json!({
                "ecrRepositoryPrefix": "mirror",
                "upstreamRegistryUrl": "public.ecr.aws",
                "upstreamRegistry": "mystery",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_rejects_url_kind_mismatch() {
        let state = EcrState::default();
        let err = create_pull_through_cache_rule(
            &state,
            &json!({
                "ecrRepositoryPrefix": "mirror",
                "upstreamRegistryUrl": "public.ecr.aws",
                "upstreamRegistry": "docker-hub",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("inferred"), "{err:?}");
    }

    #[test]
    fn create_accepts_each_documented_upstream() {
        for (url, kind) in [
            ("public.ecr.aws", "ecr-public"),
            ("https://docker.io", "docker-hub"),
            ("quay.io", "quay"),
            ("registry.k8s.io", "k8s"),
            ("ghcr.io", "github-container-registry"),
            ("myorg.azurecr.io", "azure-container-registry"),
            ("registry.gitlab.com", "gitlab-container-registry"),
            ("000000000000.dkr.ecr.us-east-1.amazonaws.com", "ecr"),
        ] {
            let state = EcrState::default();
            create_pull_through_cache_rule(
                &state,
                &json!({
                    "ecrRepositoryPrefix": format!("mirror-{kind}"),
                    "upstreamRegistryUrl": url,
                    "upstreamRegistry": kind,
                }),
                &ctx(),
            )
            .unwrap_or_else(|e| panic!("upstream `{url}` / `{kind}` rejected: {e:?}"));
        }
    }

    #[test]
    fn infer_pull_through_kind_normalizes_scheme_and_case() {
        assert_eq!(
            infer_pull_through_kind("HTTPS://PUBLIC.ECR.AWS/some/path"),
            Some("ecr-public")
        );
    }
}
