use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::operations::repositories::now_epoch_str;
use crate::state::{ContainerImage, EcrState};

const ECR_LAYER_GROUP: &str = "ecr";

fn extract_layer_digests(manifest: &str) -> Vec<String> {
    let parsed: Value = match serde_json::from_str(manifest) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut digests: Vec<String> = Vec::new();
    if let Some(layers) = parsed.get("layers").and_then(|v| v.as_array()) {
        for layer in layers {
            if let Some(d) = layer.get("digest").and_then(|v| v.as_str()) {
                digests.push(d.to_string());
            } else if let Some(d) = layer.get("blobSum").and_then(|v| v.as_str()) {
                digests.push(d.to_string());
            }
        }
    }
    if let Some(fs_layers) = parsed.get("fsLayers").and_then(|v| v.as_array()) {
        for layer in fs_layers {
            if let Some(d) = layer.get("blobSum").and_then(|v| v.as_str()) {
                digests.push(d.to_string());
            }
        }
    }
    digests
}

fn image_to_json(img: &ContainerImage, repo_name: &str, registry_id: &str) -> Value {
    let mut id = json!({
        "imageDigest": img.image_digest,
    });
    if let Some(tag) = &img.image_tag {
        id["imageTag"] = json!(tag);
    }
    let mut obj = json!({
        "registryId": registry_id,
        "repositoryName": repo_name,
        "imageId": id,
        "imageManifest": img.image_manifest,
    });
    if let Some(ref mt) = img.image_manifest_media_type {
        obj["imageManifestMediaType"] = json!(mt);
    }
    obj
}

/// Identify the manifest's canonical media type. Detects Docker schema 1
/// (signed), Docker schema 2 (manifest or list), and OCI image manifest /
/// image index. Returns Ok(None) if the manifest is structurally valid
/// JSON but lacks recognizable schema markers (the caller may still
/// declare it via imageManifestMediaType); returns InvalidParameterException
/// for non-JSON or non-object manifests.
fn detect_manifest_media_type(manifest: &str) -> Result<Option<String>, AwsError> {
    let parsed: Value = serde_json::from_str(manifest).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("imageManifest is not valid JSON: {e}"),
        )
    })?;
    let obj = parsed.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "imageManifest must be a JSON object.",
        )
    })?;

    if let Some(mt) = obj.get("mediaType").and_then(Value::as_str) {
        return Ok(Some(mt.to_string()));
    }
    match obj.get("schemaVersion").and_then(Value::as_i64) {
        Some(1) => Ok(Some(
            "application/vnd.docker.distribution.manifest.v1+json".to_string(),
        )),
        Some(2) => {
            if obj.contains_key("manifests") {
                Ok(Some(
                    "application/vnd.docker.distribution.manifest.list.v2+json".to_string(),
                ))
            } else {
                Ok(Some(
                    "application/vnd.docker.distribution.manifest.v2+json".to_string(),
                ))
            }
        }
        _ => Ok(None),
    }
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

    // Determine the manifest media type. Prefer the caller's explicit
    // `imageManifestMediaType` (used by clients that produce manifests
    // without a top-level mediaType field), otherwise parse the
    // manifest JSON and detect Docker schema 1/2 or OCI image
    // manifest/index. A malformed manifest is an InvalidParameter.
    let declared_media_type = input["imageManifestMediaType"].as_str().map(str::to_string);
    let detected_media_type = detect_manifest_media_type(manifest)?;
    let image_manifest_media_type = declared_media_type.or(detected_media_type);

    let mut repo = state.repositories.get_mut(repo_name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    // Compute digest from manifest. AWS guarantees the digest is
    // content-addressable: sha256 of the canonical manifest bytes.
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
        return Err(AwsError::bad_request(
            "ImageTagAlreadyExistsException",
            format!("An image with tag '{tag}' already exists in the repository '{repo_name}'"),
        ));
    }

    // Pushing the exact same manifest content (== same digest) without
    // a new tag is the AWS-defined collision case: real ECR returns
    // ImageAlreadyExistsException to make replays idempotent-or-loud.
    // We surface that only when the digest matches an existing image
    // that already has the same tag (or both are tagless), otherwise
    // re-tagging an existing manifest is fine.
    if repo
        .images
        .iter()
        .any(|img| img.image_digest == digest && img.image_tag.as_deref() == image_tag.as_deref())
    {
        return Err(AwsError::bad_request(
            "ImageAlreadyExistsException",
            format!(
                "Image with digest {digest} already exists in repository {repo_name}{}",
                image_tag
                    .as_deref()
                    .map(|t| format!(" under tag {t}"))
                    .unwrap_or_default()
            ),
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
        image_manifest_media_type: image_manifest_media_type.clone(),
    };

    let image_json = image_to_json(&img, repo_name, &ctx.account_id);
    repo.images.push(img);
    drop(repo);

    // Enqueue cross-region/account replication for this newly-put image
    // per the registry's replication rules (no-op when none configured).
    crate::operations::registry::enqueue_replication_for_image(
        state,
        &ctx.account_id,
        &ctx.region,
        repo_name,
        &digest,
    );

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
        AwsError::bad_request(
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
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let mut deleted_ids = Vec::new();
    let mut failures = Vec::new();
    let mut removed_manifests: Vec<String> = Vec::new();

    for id in image_ids {
        let tag = id["imageTag"].as_str();
        let digest = id["imageDigest"].as_str();

        let mut removed_here: Vec<String> = Vec::new();
        let before_len = repo.images.len();
        repo.images.retain(|img| {
            let matches = if let Some(t) = tag {
                img.image_tag.as_deref() == Some(t)
            } else if let Some(d) = digest {
                img.image_digest == d
            } else {
                false
            };
            if matches {
                removed_here.push(img.image_manifest.clone());
            }
            !matches
        });

        if repo.images.len() < before_len {
            deleted_ids.push(id.clone());
            removed_manifests.extend(removed_here);
        } else {
            failures.push(json!({
                "imageId": id,
                "failureCode": "ImageNotFound",
                "failureReason": "Requested image not found"
            }));
        }
    }

    let mut digests_to_remove: Vec<String> = Vec::new();
    for manifest in &removed_manifests {
        digests_to_remove.extend(extract_layer_digests(manifest));
    }
    for digest in &digests_to_remove {
        repo.layers.remove(digest);
    }

    drop(repo);

    if !digests_to_remove.is_empty()
        && let Some(bs) = state.body_store()
    {
        for digest in &digests_to_remove {
            if let Err(e) = bs.delete_blob(ECR_LAYER_GROUP, repo_name, digest) {
                warn!(repo = repo_name, digest = %digest, error = %e, "Failed to delete ECR layer blob");
            }
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
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let max_results = cap_max_results(input["maxResults"].as_i64(), 100, 1000);
    let mut items: Vec<(String, Value)> = repo
        .images
        .iter()
        .map(|img| {
            let mut id = json!({ "imageDigest": img.image_digest });
            if let Some(tag) = &img.image_tag {
                id["imageTag"] = json!(tag);
            }
            let key = format!(
                "{}|{}",
                img.image_digest,
                img.image_tag.as_deref().unwrap_or("")
            );
            (key, id)
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let page = paginate(items, max_results, input["nextToken"].as_str(), |(k, _)| {
        k.clone()
    })?;
    let image_ids: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    let mut resp = json!({ "imageIds": image_ids });
    if let Some(token) = page.next_token {
        resp["nextToken"] = json!(token);
    }
    Ok(resp)
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
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    if let Some(ids) = input["imageIds"].as_array() {
        let details: Vec<Value> = ids
            .iter()
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
            .collect();
        return Ok(json!({ "imageDetails": details }));
    }

    let max_results = cap_max_results(input["maxResults"].as_i64(), 100, 1000);
    let mut items: Vec<(String, Value)> = repo
        .images
        .iter()
        .map(|img| {
            let key = format!(
                "{}|{}",
                img.image_digest,
                img.image_tag.as_deref().unwrap_or("")
            );
            (key, image_detail_to_json(img, repo_name, &ctx.account_id))
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let page = paginate(items, max_results, input["nextToken"].as_str(), |(k, _)| {
        k.clone()
    })?;
    let details: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    let mut resp = json!({ "imageDetails": details });
    if let Some(token) = page.next_token {
        resp["nextToken"] = json!(token);
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// DescribeImageReplicationStatus
// ---------------------------------------------------------------------------

/// Report the replication status of a single image to each destination.
///
/// AWS keys this off the image's identity in the source repository and
/// returns one `replicationStatuses[]` entry per destination
/// (`region` + `registryId` + `status`). The status mirrors the
/// in-flight [`crate::state::ReplicationTask`] state machine
/// (PENDING -> IN_PROGRESS -> COMPLETE); destinations with no enqueued
/// task simply do not appear.
pub fn describe_image_replication_status(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })?;
    let image_id = &input["imageId"];
    let tag = image_id["imageTag"].as_str();
    let digest = image_id["imageDigest"].as_str();

    let repo = state.repositories.get(repo_name).ok_or_else(|| {
        AwsError::bad_request(
            "RepositoryNotFoundException",
            format!("The repository with name '{repo_name}' does not exist"),
        )
    })?;

    let image = repo
        .images
        .iter()
        .find(|img| {
            if let Some(t) = tag {
                img.image_tag.as_deref() == Some(t)
            } else if let Some(d) = digest {
                img.image_digest == d
            } else {
                false
            }
        })
        .ok_or_else(|| {
            AwsError::bad_request(
                "ImageNotFoundException",
                format!("The image requested does not exist in the repository '{repo_name}'"),
            )
        })?;

    let image_digest = image.image_digest.clone();
    drop(repo);

    let statuses: Vec<Value> = state
        .replication_tasks
        .iter()
        .filter(|t| t.source_repo == repo_name && t.image_digest == image_digest)
        .map(|t| {
            json!({
                "region": t.dest_region,
                "registryId": t.dest_account,
                "status": t.status,
            })
        })
        .collect();

    Ok(json!({
        "repositoryName": repo_name,
        "imageId": { "imageDigest": image_digest },
        "registryId": ctx.account_id,
        "replicationStatuses": statuses,
    }))
}

#[cfg(test)]
mod manifest_media_type_tests {
    use super::detect_manifest_media_type;

    #[test]
    fn detects_explicit_media_type_field() {
        let mt = detect_manifest_media_type(
            r#"{"mediaType":"application/vnd.oci.image.manifest.v1+json","layers":[]}"#,
        )
        .unwrap();
        assert_eq!(
            mt.as_deref(),
            Some("application/vnd.oci.image.manifest.v1+json"),
        );
    }

    #[test]
    fn detects_schema_2_manifest_without_media_type() {
        let mt = detect_manifest_media_type(r#"{"schemaVersion":2,"layers":[]}"#).unwrap();
        assert_eq!(
            mt.as_deref(),
            Some("application/vnd.docker.distribution.manifest.v2+json"),
        );
    }

    #[test]
    fn detects_schema_2_manifest_list() {
        let mt = detect_manifest_media_type(r#"{"schemaVersion":2,"manifests":[]}"#).unwrap();
        assert_eq!(
            mt.as_deref(),
            Some("application/vnd.docker.distribution.manifest.list.v2+json"),
        );
    }

    #[test]
    fn detects_schema_1() {
        let mt = detect_manifest_media_type(r#"{"schemaVersion":1,"fsLayers":[]}"#).unwrap();
        assert_eq!(
            mt.as_deref(),
            Some("application/vnd.docker.distribution.manifest.v1+json"),
        );
    }

    #[test]
    fn rejects_non_json_manifest() {
        let err = detect_manifest_media_type("not-json").unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_non_object_json_manifest() {
        let err = detect_manifest_media_type("[]").unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }
}
