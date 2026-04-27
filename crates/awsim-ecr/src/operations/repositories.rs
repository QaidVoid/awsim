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

    if state.repositories.contains_key(name) {
        return Err(AwsError::conflict(
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
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{name}' does not exist in the registry"),
        )
    })?;

    if !force && !repo.images.is_empty() {
        return Err(AwsError::conflict(
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
                AwsError::not_found(
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
