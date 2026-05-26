use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, Body, RequestContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use tracing::warn;

use crate::state::{EcrState, Layer, LayerUpload};

const DEFAULT_LAYER_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";
const ECR_LAYER_GROUP: &str = "ecr";

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
    AwsError::bad_request(
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
        AwsError::bad_request(
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
        AwsError::bad_request(
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

    // AWS rejects non-JSON or structurally-invalid policy documents
    // up front with InvalidParameterException — the document must be a
    // JSON object with a `Statement` array.
    let parsed: Value = serde_json::from_str(policy_text).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("policyText is not valid JSON: {e}"),
        )
    })?;
    if !parsed.is_object() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "policyText must be a JSON object.",
        ));
    }
    if !parsed
        .get("Statement")
        .map(|s| s.is_array())
        .unwrap_or(false)
    {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "policyText must include a Statement array.",
        ));
    }

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
        AwsError::bad_request(
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
        AwsError::bad_request(
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
        return Err(AwsError::bad_request(
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

    // ENHANCED scanning surfaces a CVE-shaped finding list; BASIC just
    // returns severity counts. We synthesize deterministic example
    // findings for ENHANCED so SDKs that key off `enhancedFindings`
    // see the expected shape, and emit only severity counts for BASIC.
    let scan_type = state
        .registry_scanning_config
        .read()
        .ok()
        .map(|c| c.scan_type.clone())
        .unwrap_or_default();
    let is_enhanced = scan_type == "ENHANCED";

    let findings = Value::Array(vec![]);
    let enhanced_findings = if is_enhanced {
        Value::Array(vec![json!({
            "title": "CVE-2024-EXAMPLE-0001",
            "description": "Synthetic finding emitted by awsim ENHANCED scanning.",
            "severity": "MEDIUM",
            "packageVulnerabilityDetails": {
                "source": "AWSIM",
                "vulnerabilityId": "CVE-2024-EXAMPLE-0001",
            },
            "remediation": {
                "recommendation": { "text": "Upgrade to the latest patched version." }
            }
        })])
    } else {
        Value::Array(vec![])
    };
    let severity_counts = if is_enhanced {
        json!({ "MEDIUM": 1 })
    } else {
        json!({})
    };

    Ok(json!({
        "registryId": ctx.account_id,
        "repositoryName": repo_name,
        "imageId": image_id,
        "imageScanStatus": {
            "status": "COMPLETE",
            "description": "The scan is complete for the specified image.",
        },
        "imageScanFindings": {
            "findings": findings,
            "enhancedFindings": enhanced_findings,
            "findingSeverityCounts": severity_counts,
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
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let repo_name = require_repo_name(input)?;
    let layer_digest = input["layerDigest"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "layerDigest is required")
    })?;

    let repo = state
        .repositories
        .get(repo_name)
        .ok_or_else(|| repo_not_found(repo_name))?;

    if !repo.layers.contains_key(layer_digest) {
        return Err(AwsError::bad_request(
            "LayersNotFoundException",
            format!("Layer with digest '{layer_digest}' does not exist in the repository"),
        ));
    }

    let port = state.port.load(std::sync::atomic::Ordering::Relaxed);

    Ok(json!({
        "downloadUrl": format!(
            "http://localhost:{port}/v2/{repo_name}/blobs/{layer_digest}"
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
        AwsError::bad_request(
            "UploadNotFoundException",
            format!("Upload session '{upload_id}' not found"),
        )
    })?;

    // AWS verifies that the part is contiguous with what the server has
    // already received: `partFirstByte` must equal the current
    // last-byte-received, and `partLastByte` must equal
    // partFirstByte + len(layerPartBlob) - 1. Mismatches return
    // InvalidLayerPartException so the client can recover by resuming
    // from `lastByteReceived`.
    let current = upload.part_data.len() as u64;
    let supplied_first = input.get("partFirstByte").and_then(Value::as_u64);
    let supplied_last = input.get("partLastByte").and_then(Value::as_u64);
    if let Some(first) = supplied_first
        && first != current
    {
        return Err(AwsError::bad_request(
            "InvalidLayerPartException",
            format!(
                "partFirstByte {first} does not match the upload's lastByteReceived ({current}); \
                 resume from that offset."
            ),
        ));
    }
    if let (Some(first), Some(last)) = (supplied_first, supplied_last) {
        let expected_last = first + part_data.len() as u64 - 1;
        if last != expected_last {
            return Err(AwsError::bad_request(
                "InvalidLayerPartException",
                format!(
                    "partLastByte {last} does not match partFirstByte {first} + payload length \
                     ({}) - 1.",
                    part_data.len()
                ),
            ));
        }
    }

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
        AwsError::bad_request(
            "UploadNotFoundException",
            format!("Upload session '{upload_id}' not found"),
        )
    })?;

    let bytes = upload.part_data;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let layer_digest = format!("sha256:{:x}", hasher.finalize());
    let size = bytes.len() as u64;

    let body = match state.body_store() {
        Some(bs) => match bs.write_blob(ECR_LAYER_GROUP, repo_name, &layer_digest, &bytes) {
            Ok(path) => Body::OnDisk(path),
            Err(e) => {
                warn!(repo = repo_name, digest = %layer_digest, error = %e, "Failed to persist ECR layer; falling back to in-memory");
                Body::InMemory(bytes)
            }
        },
        None => Body::InMemory(bytes),
    };

    let layer = Layer {
        digest: layer_digest.clone(),
        body,
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
