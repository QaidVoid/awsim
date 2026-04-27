use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::state::{EcrState, Layer, LayerBody, LayerUpload};

const DEFAULT_LAYER_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";

fn now_epoch_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

fn require_repo_name(input: &Value) -> Result<&str, AwsError> {
    input["repositoryName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "repositoryName is required")
    })
}

fn repo_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "RepositoryNotFoundException",
        format!("The repository with name '{name}' does not exist in the registry"),
    )
}

// ---------------------------------------------------------------------------
// Lifecycle Policy
// ---------------------------------------------------------------------------

pub fn put_lifecycle_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let policy = input["lifecyclePolicyText"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "lifecyclePolicyText is required",
        )
    })?;

    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;
    repo.lifecycle_policy = Some(policy.to_string());

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": policy,
    }))
}

pub fn get_lifecycle_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let policy = repo.lifecycle_policy.as_deref().ok_or_else(|| {
        AwsError::not_found(
            "LifecyclePolicyNotFoundException",
            format!("Lifecycle policy for repository '{repo_name}' not found"),
        )
    })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": policy,
        "lastEvaluatedAt": now_epoch_str(),
    }))
}

pub fn delete_lifecycle_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let policy = repo.lifecycle_policy.take().ok_or_else(|| {
        AwsError::not_found(
            "LifecyclePolicyNotFoundException",
            format!("Lifecycle policy for repository '{repo_name}' not found"),
        )
    })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "lifecyclePolicyText": policy,
    }))
}

// ---------------------------------------------------------------------------
// Repository Policy
// ---------------------------------------------------------------------------

pub fn set_repository_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let policy_text = input["policyText"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "policyText is required")
    })?;

    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;
    repo.repository_policy = Some(policy_text.to_string());

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "policyText": policy_text,
    }))
}

pub fn get_repository_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let policy = repo.repository_policy.as_deref().ok_or_else(|| {
        AwsError::not_found(
            "RepositoryPolicyNotFoundException",
            format!("Repository policy for repository '{repo_name}' not found"),
        )
    })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "policyText": policy,
    }))
}

pub fn delete_repository_policy(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let mut repo = state
        .repositories
        .get_mut(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let policy = repo.repository_policy.take().ok_or_else(|| {
        AwsError::not_found(
            "RepositoryPolicyNotFoundException",
            format!("Repository policy for repository '{repo_name}' not found"),
        )
    })?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "policyText": policy,
    }))
}

// ---------------------------------------------------------------------------
// Image Scanning
// ---------------------------------------------------------------------------

pub fn start_image_scan(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let image_id = &input["imageId"];

    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let tag = image_id["imageTag"].as_str();
    let digest = image_id["imageDigest"].as_str();

    let image_exists = repo.images.iter().any(|img| {
        if let Some(t) = tag {
            img.image_tag.as_deref() == Some(t)
        } else if let Some(d) = digest {
            img.image_digest == d
        } else {
            false
        }
    });

    if !image_exists {
        return Err(AwsError::not_found(
            "ImageNotFoundException",
            format!("The image requested does not exist in the repository '{repo_name}'"),
        ));
    }

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "imageId": image_id,
        "imageScanStatus": {
            "status": "COMPLETE",
            "description": "The scan is complete for the specified image.",
        },
    }))
}

pub fn describe_image_scan_findings(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let image_id = &input["imageId"];

    let _repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "imageId": image_id,
        "imageScanStatus": {
            "status": "COMPLETE",
            "description": "The scan is complete for the specified image.",
        },
        "imageScanFindings": {
            "findings": [],
            "findingSeverityCounts": {},
            "imageScanCompletedAt": now_epoch_str(),
            "vulnerabilitySourceUpdatedAt": now_epoch_str(),
        },
        "nextToken": null,
    }))
}

// ---------------------------------------------------------------------------
// GetDownloadUrlForLayer
// ---------------------------------------------------------------------------

pub fn get_download_url_for_layer(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let layer_digest = input["layerDigest"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "layerDigest is required")
    })?;

    let _repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    Ok(json!({
        "downloadUrl": format!(
            "http://ecr.{}.amazonaws.com/download/{}/{}/{}",
            ctx.region, ctx.account_id, repo_name, layer_digest
        ),
        "layerDigest": layer_digest,
    }))
}

// ---------------------------------------------------------------------------
// BatchCheckLayerAvailability
// ---------------------------------------------------------------------------

pub fn batch_check_layer_availability(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let layer_digests = input["layerDigests"].as_array().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "layerDigests is required")
    })?;

    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let mut layers: Vec<Value> = Vec::new();
    let mut failures: Vec<Value> = Vec::new();

    for d in layer_digests.iter().filter_map(|d| d.as_str()) {
        match repo.layers.get(d) {
            Some(layer) => {
                layers.push(json!({
                    "layerDigest": d,
                    "layerAvailability": "AVAILABLE",
                    "layerSize": layer.size,
                    "mediaType": layer.media_type,
                }));
            }
            None => {
                failures.push(json!({
                    "layerDigest": d,
                    "failureCode": "MissingLayerDigest",
                    "failureReason": "Layer not found",
                }));
            }
        }
    }

    Ok(json!({
        "layers": layers,
        "failures": failures,
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
    }))
}

// ---------------------------------------------------------------------------
// Layer Upload (InitiateLayerUpload / UploadLayerPart / CompleteLayerUpload)
// ---------------------------------------------------------------------------

pub fn initiate_layer_upload(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let _repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let upload_id = new_uuid();

    state.layer_uploads.insert(
        upload_id.clone(),
        LayerUpload {
            upload_id: upload_id.clone(),
            repository_name: repo_name.to_string(),
            part_data: Vec::new(),
        },
    );

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "uploadId": upload_id,
        "lastByteReceived": 0u64,
    }))
}

pub fn upload_layer_part(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let upload_id = input["uploadId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "uploadId is required")
    })?;

    // Accept part data (base64 encoded layer data)
    let part_data = input["layerPartBlob"].as_str().unwrap_or("").as_bytes();

    let mut upload = state.layer_uploads.get_mut(upload_id).ok_or_else(|| {
        AwsError::not_found(
            "UploadNotFoundException",
            format!("Upload session '{upload_id}' not found"),
        )
    })?;

    upload.part_data.extend_from_slice(part_data);
    let last_byte = upload.part_data.len() as u64;

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "uploadId": upload_id,
        "lastByteReceived": last_byte,
    }))
}

pub fn complete_layer_upload(
    state: &EcrState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let upload_id = input["uploadId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "uploadId is required")
    })?;

    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    let (_, upload) = state.layer_uploads.remove(upload_id).ok_or_else(|| {
        AwsError::not_found(
            "UploadNotFoundException",
            format!("Upload session '{upload_id}' not found"),
        )
    })?;

    let bytes = upload.part_data;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let layer_digest = format!("sha256:{:x}", hasher.finalize());
    let size = bytes.len() as u64;

    let layer = Layer {
        digest: layer_digest.clone(),
        body: LayerBody::InMemory(bytes),
        size,
        media_type: DEFAULT_LAYER_MEDIA_TYPE.to_string(),
    };
    repo.layers.insert(layer_digest.clone(), layer);

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "uploadId": upload_id,
        "layerDigest": layer_digest,
    }))
}
