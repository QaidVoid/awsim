use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::state::{EcrState, Repository};

const ECR_LAYER_GROUP: &str = "ecr";

pub fn now_epoch_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn repo_to_json(repo: &Repository) -> Value {
    json!({
        "repositoryName": repo.name,
        "repositoryArn": repo.arn,
        "registryId": repo.registry_id,
        "repositoryUri": repo.repository_uri,
        "createdAt": repo.created_at,
        "imageTagMutability": repo.image_tag_mutability,
        "imageScanningConfiguration": {
            "scanOnPush": repo.scan_on_push
        },
        "encryptionConfiguration": {
            "encryptionType": "AES256"
        }
    })
}

// ---------------------------------------------------------------------------
// CreateRepository
// ---------------------------------------------------------------------------

pub fn create_repository(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;
    validate_repository_name(name)?;

    if state.repositories.contains_key(name) {
        return Err(AwsError::bad_request(
            "RepositoryAlreadyExistsException",
            format!(
                "The repository with name '{name}' already exists in the registry with id '{}'",
                ctx.account_id
            ),
        ));
    }

    let image_tag_mutability = input["imageTagMutability"]
        .as_str()
        .unwrap_or("MUTABLE")
        .to_string();
    if !matches!(image_tag_mutability.as_str(), "MUTABLE" | "IMMUTABLE") {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("imageTagMutability '{image_tag_mutability}' must be MUTABLE or IMMUTABLE."),
        ));
    }

    let arn = format!(
        "arn:aws:ecr:{}:{}:repository/{}",
        ctx.region, ctx.account_id, name
    );
    let repository_uri = format!(
        "{}.dkr.ecr.{}.localhost/{}",
        ctx.account_id, ctx.region, name
    );

    let mut tags = std::collections::HashMap::new();
    if let Some(tag_list) = input["tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    let scan_on_push = input["imageScanningConfiguration"]["scanOnPush"]
        .as_bool()
        .unwrap_or(false);

    let repo = Repository {
        name: name.to_string(),
        arn: arn.clone(),
        registry_id: ctx.account_id.clone(),
        repository_uri,
        images: Vec::new(),
        layers: dashmap::DashMap::new(),
        created_at: now_epoch_str(),
        image_tag_mutability,
        tags,
        lifecycle_policy: None,
        lifecycle_policy_preview: None,
        repository_policy: None,
        scan_on_push,
    };

    info!(repository = %name, "Created ECR repository");
    let repo_json = repo_to_json(&repo);
    state.repositories.insert(name.to_string(), repo);

    Ok(json!({ "repository": repo_json }))
}

// ---------------------------------------------------------------------------
// DeleteRepository
// ---------------------------------------------------------------------------

pub fn delete_repository(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let force = input["force"].as_bool().unwrap_or(false);

    let repo = state.repositories.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{name}' does not exist in the registry"),
        )
    })?;

    if !force && !repo.images.is_empty() {
        return Err(AwsError::bad_request(
            "RepositoryNotEmptyException",
            format!("The repository with name '{name}' is not empty"),
        ));
    }

    let repo_json = repo_to_json(&repo);
    drop(repo);

    state.repositories.remove(name);

    if let Some(bs) = state.body_store()
        && let Err(e) = bs.delete_bucket(ECR_LAYER_GROUP, name)
    {
        warn!(repository = %name, error = %e, "Failed to delete ECR layer bucket on disk");
    }

    info!(repository = %name, "Deleted ECR repository");

    Ok(json!({ "repository": repo_json }))
}

// ---------------------------------------------------------------------------
// DescribeRepositories
// ---------------------------------------------------------------------------

pub fn describe_repositories(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repos: Vec<Value> = if let Some(names) = input["repositoryNames"].as_array() {
        let mut result = Vec::new();
        for name_val in names {
            let name = name_val.as_str().unwrap_or("");
            let repo = state.repositories.get(name).ok_or_else(|| {
                AwsError::bad_request(
                    "RepositoryNotFoundException",
                    format!("The repository with name '{name}' does not exist in the registry"),
                )
            })?;
            result.push(repo_to_json(&repo));
        }
        result
    } else {
        state
            .repositories
            .iter()
            .map(|entry| repo_to_json(entry.value()))
            .collect()
    };

    Ok(json!({ "repositories": repos }))
}

/// Validate an ECR repository name against AWS's regex.
///
/// AWS documents the pattern as
/// `(?:[a-z0-9]+(?:[._-][a-z0-9]+)*/)*[a-z0-9]+(?:[._-][a-z0-9]+)*`
/// with length 2-256. Uppercase letters and most punctuation are
/// rejected. Without this check, a caller can register
/// `MyRepo` here that real ECR refuses on first push.
fn validate_repository_name(name: &str) -> Result<(), AwsError> {
    if name.len() < 2 || name.len() > 256 {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "repositoryName length must be between 2 and 256, got {}.",
                name.len()
            ),
        ));
    }
    // Each path segment: starts with [a-z0-9], optionally followed by
    // pairs of ([._-] then [a-z0-9]). Path separator is /.
    for segment in name.split('/') {
        if segment.is_empty() {
            return Err(reject(name));
        }
        let bytes = segment.as_bytes();
        let first = bytes[0];
        if !is_lower_alnum(first) {
            return Err(reject(name));
        }
        let mut i = 1;
        while i < bytes.len() {
            let b = bytes[i];
            if is_lower_alnum(b) {
                i += 1;
                continue;
            }
            if matches!(b, b'.' | b'_' | b'-') {
                if i + 1 >= bytes.len() || !is_lower_alnum(bytes[i + 1]) {
                    return Err(reject(name));
                }
                i += 2;
                continue;
            }
            return Err(reject(name));
        }
    }
    Ok(())
}

fn is_lower_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

fn reject(name: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidParameterException",
        format!(
            "repositoryName '{name}' is invalid. Must match the pattern \
             (?:[a-z0-9]+(?:[._-][a-z0-9]+)*/)*[a-z0-9]+(?:[._-][a-z0-9]+)*."
        ),
    )
}

#[cfg(test)]
mod repository_name_tests {
    use super::*;

    #[test]
    fn accepts_simple_lowercase() {
        validate_repository_name("my-repo").unwrap();
        validate_repository_name("nginx").unwrap();
        validate_repository_name("ab").unwrap();
    }

    #[test]
    fn accepts_nested_paths() {
        validate_repository_name("team/service").unwrap();
        validate_repository_name("apps/web/frontend").unwrap();
    }

    #[test]
    fn accepts_documented_separators() {
        validate_repository_name("a.b").unwrap();
        validate_repository_name("a_b").unwrap();
        validate_repository_name("a-b").unwrap();
        validate_repository_name("a.b_c-d").unwrap();
    }

    #[test]
    fn rejects_uppercase() {
        assert!(validate_repository_name("MyRepo").is_err());
        assert!(validate_repository_name("nginX").is_err());
    }

    #[test]
    fn rejects_leading_or_trailing_separator() {
        assert!(validate_repository_name("-repo").is_err());
        assert!(validate_repository_name("repo-").is_err());
        assert!(validate_repository_name(".repo").is_err());
    }

    #[test]
    fn rejects_consecutive_separators() {
        assert!(validate_repository_name("a__b").is_err());
        assert!(validate_repository_name("a..b").is_err());
    }

    #[test]
    fn rejects_empty_segment() {
        assert!(validate_repository_name("a//b").is_err());
        assert!(validate_repository_name("/foo").is_err());
    }

    #[test]
    fn rejects_too_short_or_too_long() {
        assert!(validate_repository_name("a").is_err());
        let long = "a".repeat(257);
        assert!(validate_repository_name(&long).is_err());
    }
}
