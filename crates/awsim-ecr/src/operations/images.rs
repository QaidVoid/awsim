use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::info;

use crate::operations::repositories::now_epoch_str;
use crate::state::{ContainerImage, EcrState};

fn image_to_json(img: &ContainerImage, repo_name: &str, registry_id: &str) -> Value {
    let mut id = json!({
        "imageDigest": img.image_digest,
    });
    if let Some(tag) = &img.image_tag {
        id["imageTag"] = json!(tag);
    }
    json!({
        "registryId": registry_id,
        "repositoryName": repo_name,
        "imageId": id,
        "imageManifest": img.image_manifest,
    })
}

fn image_detail_to_json(img: &ContainerImage, repo_name: &str, registry_id: &str) -> Value {
    let mut detail = json!({
        "registryId": registry_id,
        "repositoryName": repo_name,
        "imageDigest": img.image_digest,
        "imageSizeInBytes": img.image_size_in_bytes,
        "imagePushedAt": img.pushed_at,
        "imageScanStatus": { "status": "COMPLETE" },
        "imageTags": [],
    });
    if let Some(tag) = &img.image_tag {
        detail["imageTags"] = json!([tag]);
    }
    detail
}

// ---------------------------------------------------------------------------
// PutImage
// ---------------------------------------------------------------------------

pub fn put_image(state: &EcrState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let manifest = input["imageManifest"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "imageManifest is required")
    })?;

    let image_tag = input["imageTag"].as_str().map(|s| s.to_string());

    let mut repo = state.repositories.get_mut(repo_name).ok_or_else(|| {
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    // Compute digest from manifest
    let mut hasher = Sha256::new();
    hasher.update(manifest.as_bytes());
    let digest = format!("sha256:{:x}", hasher.finalize());

    // If tag mutability is IMMUTABLE and tag already exists, error
    if repo.image_tag_mutability == "IMMUTABLE"
        && let Some(ref tag) = image_tag
        && repo
            .images
            .iter()
            .any(|img| img.image_tag.as_deref() == Some(tag))
    {
        return Err(AwsError::conflict(
            "ImageTagAlreadyExistsException",
            format!("An image with tag '{tag}' already exists in the repository '{repo_name}'"),
        ));
    }

    // Remove existing image with same tag (if mutable)
    if let Some(ref tag) = image_tag {
        repo.images
            .retain(|img| img.image_tag.as_deref() != Some(tag));
    }

    let size = manifest.len() as u64;
    let img = ContainerImage {
        image_digest: digest.clone(),
        image_tag: image_tag.clone(),
        image_manifest: manifest.to_string(),
        pushed_at: now_epoch_str(),
        image_size_in_bytes: size,
    };

    let image_json = image_to_json(&img, repo_name, &ctx.account_id);
    repo.images.push(img);

    info!(repository = %repo_name, digest = %digest, "Put ECR image");

    Ok(json!({ "image": image_json }))
}

// ---------------------------------------------------------------------------
// BatchGetImage
// ---------------------------------------------------------------------------

pub fn batch_get_image(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let image_ids = input["imageIds"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "imageIds is required")
    })?;

    let repo = state.repositories.get(repo_name).ok_or_else(|| {
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let mut images = Vec::new();
    let mut failures = Vec::new();

    for id in image_ids {
        let tag = id["imageTag"].as_str();
        let digest = id["imageDigest"].as_str();

        let found = repo.images.iter().find(|img| {
            if let Some(t) = tag {
                img.image_tag.as_deref() == Some(t)
            } else if let Some(d) = digest {
                img.image_digest == d
            } else {
                false
            }
        });

        match found {
            Some(img) => images.push(image_to_json(img, repo_name, &ctx.account_id)),
            None => {
                failures.push(json!({
                    "imageId": id,
                    "failureCode": "ImageNotFound",
                    "failureReason": "Requested image not found"
                }));
            }
        }
    }

    Ok(json!({ "images": images, "failures": failures }))
}

// ---------------------------------------------------------------------------
// BatchDeleteImage
// ---------------------------------------------------------------------------

pub fn batch_delete_image(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let image_ids = input["imageIds"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "imageIds is required")
    })?;

    let mut repo = state.repositories.get_mut(repo_name).ok_or_else(|| {
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let mut deleted_ids = Vec::new();
    let mut failures = Vec::new();

    for id in image_ids {
        let tag = id["imageTag"].as_str();
        let digest = id["imageDigest"].as_str();

        let before_len = repo.images.len();
        repo.images.retain(|img| {
            let matches = if let Some(t) = tag {
                img.image_tag.as_deref() == Some(t)
            } else if let Some(d) = digest {
                img.image_digest == d
            } else {
                false
            };
            !matches
        });

        if repo.images.len() < before_len {
            deleted_ids.push(id.clone());
        } else {
            failures.push(json!({
                "imageId": id,
                "failureCode": "ImageNotFound",
                "failureReason": "Requested image not found"
            }));
        }
    }

    Ok(json!({ "imageIds": deleted_ids, "failures": failures }))
}

// ---------------------------------------------------------------------------
// ListImages
// ---------------------------------------------------------------------------

pub fn list_images(
    state: &EcrState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let repo = state.repositories.get(repo_name).ok_or_else(|| {
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let image_ids: Vec<Value> = repo
        .images
        .iter()
        .map(|img| {
            let mut id = json!({ "imageDigest": img.image_digest });
            if let Some(tag) = &img.image_tag {
                id["imageTag"] = json!(tag);
            }
            id
        })
        .collect();

    Ok(json!({ "imageIds": image_ids }))
}

// ---------------------------------------------------------------------------
// DescribeImages
// ---------------------------------------------------------------------------

pub fn describe_images(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;

    let repo = state.repositories.get(repo_name).ok_or_else(|| {
        AwsError::not_found(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let details: Vec<Value> = if let Some(ids) = input["imageIds"].as_array() {
        ids.iter()
            .filter_map(|id| {
                let tag = id["imageTag"].as_str();
                let digest = id["imageDigest"].as_str();
                repo.images.iter().find(|img| {
                    if let Some(t) = tag {
                        img.image_tag.as_deref() == Some(t)
                    } else if let Some(d) = digest {
                        img.image_digest == d
                    } else {
                        false
                    }
                })
            })
            .map(|img| image_detail_to_json(img, repo_name, &ctx.account_id))
            .collect()
    } else {
        repo.images
            .iter()
            .map(|img| image_detail_to_json(img, repo_name, &ctx.account_id))
            .collect()
    };

    Ok(json!({ "imageDetails": details }))
}
