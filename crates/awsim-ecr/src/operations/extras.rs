use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, Body, RequestContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use tracing::warn;

use crate::state::{EcrState, Layer, LayerUpload};

const DEFAULT_LAYER_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";
const ECR_LAYER_GROUP: &str = "ecr";
/// Bucket under the `ecr` group that holds in-progress upload temp
/// blobs, keyed by uploadId, before CompleteLayerUpload finalizes them.
const UPLOADS_BUCKET: &str = "_uploads";

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

    parse_lifecycle_policy(policy)?;

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

/// Parsed representation of an ECR lifecycle policy. The DSL is a
/// JSON object with a `rules[]` array; each entry describes which
/// images to expire and the precedence among rules.
#[derive(Debug, Clone)]
pub(crate) struct LifecyclePolicy {
    pub(crate) rules: Vec<LifecycleRule>,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleRule {
    pub(crate) priority: u32,
    pub(crate) description: Option<String>,
    pub(crate) tag_status: TagStatus,
    pub(crate) tag_prefixes: Vec<String>,
    pub(crate) tag_patterns: Vec<String>,
    pub(crate) selection: LifecycleSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagStatus {
    Tagged,
    Untagged,
    Any,
}

#[derive(Debug, Clone)]
pub(crate) enum LifecycleSelection {
    /// Keep at most `count` matching images; expire the rest, oldest first.
    CountMoreThan { count: usize },
    /// Expire any image older than `days` days.
    SinceImagePushed { days: u32 },
}

/// Parse + validate a lifecycle policy text. AWS rejects malformed
/// policies at PutLifecyclePolicy time with
/// `InvalidParameterException`; mirror that so a typo doesn't sit
/// quietly until the (future) scheduler tries to evaluate it.
pub(crate) fn parse_lifecycle_policy(text: &str) -> Result<LifecyclePolicy, AwsError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        AwsError::bad_request(
            "InvalidParameterException",
            format!("lifecyclePolicyText is not valid JSON: {e}"),
        )
    })?;
    let rules = v.get("rules").and_then(Value::as_array).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "lifecyclePolicyText must contain a `rules` array.",
        )
    })?;
    if rules.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "lifecyclePolicyText.rules must contain at least one rule.",
        ));
    }
    let mut parsed = Vec::with_capacity(rules.len());
    for rule in rules {
        parsed.push(parse_rule(rule)?);
    }
    // AWS evaluates rules in ascending priority order.
    parsed.sort_by_key(|r| r.priority);
    Ok(LifecyclePolicy { rules: parsed })
}

fn parse_rule(rule: &Value) -> Result<LifecycleRule, AwsError> {
    let priority = rule
        .get("rulePriority")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_lifecycle("rule.rulePriority is required"))?;
    let description = rule
        .get("description")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let selection_obj = rule
        .get("selection")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_lifecycle("rule.selection is required"))?;
    let tag_status = match selection_obj
        .get("tagStatus")
        .and_then(Value::as_str)
        .unwrap_or("any")
    {
        "tagged" => TagStatus::Tagged,
        "untagged" => TagStatus::Untagged,
        "any" => TagStatus::Any,
        other => {
            return Err(invalid_lifecycle(format!(
                "tagStatus `{other}` must be tagged | untagged | any"
            )));
        }
    };

    let tag_prefixes: Vec<String> = selection_obj
        .get("tagPrefixList")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let tag_patterns: Vec<String> = selection_obj
        .get("tagPatternList")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if matches!(tag_status, TagStatus::Tagged) && tag_prefixes.is_empty() && tag_patterns.is_empty()
    {
        return Err(invalid_lifecycle(
            "tagStatus=tagged requires tagPrefixList or tagPatternList.",
        ));
    }
    if !tag_prefixes.is_empty() && !tag_patterns.is_empty() {
        return Err(invalid_lifecycle(
            "tagPrefixList and tagPatternList are mutually exclusive.",
        ));
    }

    let count_type = selection_obj
        .get("countType")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_lifecycle("selection.countType is required"))?;
    let count_number = selection_obj
        .get("countNumber")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_lifecycle("selection.countNumber is required"))?;
    let selection = match count_type {
        "imageCountMoreThan" => LifecycleSelection::CountMoreThan {
            count: count_number as usize,
        },
        "sinceImagePushed" => {
            let unit = selection_obj
                .get("countUnit")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid_lifecycle("countType=sinceImagePushed requires countUnit=days")
                })?;
            if unit != "days" {
                return Err(invalid_lifecycle("countUnit must be `days`"));
            }
            LifecycleSelection::SinceImagePushed {
                days: count_number as u32,
            }
        }
        other => {
            return Err(invalid_lifecycle(format!(
                "countType `{other}` must be imageCountMoreThan or sinceImagePushed"
            )));
        }
    };

    let action_type = rule
        .get("action")
        .and_then(|a| a.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("expire");
    if action_type != "expire" {
        return Err(invalid_lifecycle(format!(
            "action.type `{action_type}` is not supported (must be `expire`)"
        )));
    }

    Ok(LifecycleRule {
        priority: priority as u32,
        description,
        tag_status,
        tag_prefixes,
        tag_patterns,
        selection,
    })
}

fn invalid_lifecycle(msg: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidParameterException", msg)
}

/// Evaluate a parsed lifecycle policy against a list of images. The
/// scheduler hands the result to BatchDeleteImage. Output is the
/// list of `(image_digest, matched_rule_priority, matched_rule_description)`
/// triples — preserving order of evaluation so the trace is stable.
pub(crate) fn evaluate_lifecycle_policy(
    policy: &LifecyclePolicy,
    images: &[crate::state::ContainerImage],
    now_epoch_secs: u64,
) -> Vec<(String, u32, Option<String>)> {
    let mut expired: std::collections::BTreeMap<String, (u32, Option<String>)> =
        std::collections::BTreeMap::new();
    for rule in &policy.rules {
        let mut matching: Vec<&crate::state::ContainerImage> = images
            .iter()
            .filter(|img| !expired.contains_key(&img.image_digest))
            .filter(|img| rule_matches_tag(rule, img))
            .collect();
        match rule.selection {
            LifecycleSelection::CountMoreThan { count } => {
                // Keep newest `count`; expire the rest, oldest first.
                matching.sort_by(|a, b| b.pushed_at.cmp(&a.pushed_at));
                for img in matching.into_iter().skip(count) {
                    expired.insert(
                        img.image_digest.clone(),
                        (rule.priority, rule.description.clone()),
                    );
                }
            }
            LifecycleSelection::SinceImagePushed { days } => {
                let cutoff = now_epoch_secs.saturating_sub(u64::from(days) * 86_400);
                for img in matching {
                    let pushed: u64 = img.pushed_at.parse().unwrap_or(0);
                    if pushed <= cutoff {
                        expired.insert(
                            img.image_digest.clone(),
                            (rule.priority, rule.description.clone()),
                        );
                    }
                }
            }
        }
    }
    expired
        .into_iter()
        .map(|(digest, (priority, desc))| (digest, priority, desc))
        .collect()
}

fn rule_matches_tag(rule: &LifecycleRule, image: &crate::state::ContainerImage) -> bool {
    match rule.tag_status {
        TagStatus::Any => true,
        TagStatus::Untagged => image.image_tag.is_none(),
        TagStatus::Tagged => {
            let Some(tag) = image.image_tag.as_deref() else {
                return false;
            };
            if !rule.tag_prefixes.is_empty() {
                return rule.tag_prefixes.iter().any(|p| tag.starts_with(p));
            }
            if !rule.tag_patterns.is_empty() {
                return rule.tag_patterns.iter().any(|p| pattern_matches(p, tag));
            }
            true
        }
    }
}

/// AWS lifecycle policy tagPatternList uses simple glob with `*`.
fn pattern_matches(pattern: &str, tag: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == tag;
    }
    let mut remaining = tag;
    let parts: Vec<&str> = pattern.split('*').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
        } else {
            match remaining.find(part) {
                Some(idx) => remaining = &remaining[idx + part.len()..],
                None => return false,
            }
        }
    }
    true
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
            bytes_received: 0,
            hasher: Sha256::new(),
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
    let current = upload.bytes_received;
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

    // Stream the part to a temporary `_uploads/<upload_id>` blob instead
    // of buffering it. Keep an in-memory fallback only when no body
    // store is configured, mirroring complete_layer_upload's fallback.
    match state.body_store() {
        Some(bs) => {
            if let Err(e) = bs.append_blob(ECR_LAYER_GROUP, UPLOADS_BUCKET, upload_id, part_data) {
                warn!(upload = %upload_id, error = %e, "Failed to append ECR layer part; buffering in-memory");
                upload.part_data.extend_from_slice(part_data);
            }
        }
        None => upload.part_data.extend_from_slice(part_data),
    }
    upload.hasher.update(part_data);
    upload.bytes_received += part_data.len() as u64;
    let last_byte = upload.bytes_received;

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

    // Finalize the digest from the carried streaming hasher; the bytes
    // themselves live in the temp `_uploads/<upload_id>` blob (or the
    // in-memory fallback when no body store is configured).
    let layer_digest = format!("sha256:{:x}", upload.hasher.clone().finalize());
    let size = upload.bytes_received;

    let body = match state.body_store() {
        Some(bs) => {
            // Read the streamed temp blob and move it into the
            // digest-keyed blob the crate serves layers from, then drop
            // the temp upload. Fall back to in-memory if the temp blob
            // is unreadable (e.g. an append failed mid-upload).
            let bytes = bs
                .read_blob(ECR_LAYER_GROUP, UPLOADS_BUCKET, upload_id)
                .unwrap_or_else(|_| upload.part_data.clone());
            let body = match bs.write_blob(ECR_LAYER_GROUP, repo_name, &layer_digest, &bytes) {
                Ok(path) => Body::OnDisk(path),
                Err(e) => {
                    warn!(repo = repo_name, digest = %layer_digest, error = %e, "Failed to persist ECR layer; falling back to in-memory");
                    Body::InMemory(bytes)
                }
            };
            if let Err(e) = bs.delete_blob(ECR_LAYER_GROUP, UPLOADS_BUCKET, upload_id) {
                warn!(upload = %upload_id, error = %e, "Failed to remove ECR layer upload temp blob");
            }
            body
        }
        None => Body::InMemory(upload.part_data),
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::state::ContainerImage;

    fn image(digest: &str, tag: Option<&str>, pushed_at: u64) -> ContainerImage {
        ContainerImage {
            image_digest: digest.into(),
            image_tag: tag.map(String::from),
            image_manifest: "{}".into(),
            pushed_at: pushed_at.to_string(),
            image_size_in_bytes: 0,
            image_manifest_media_type: None,
        }
    }

    #[test]
    fn rejects_malformed_policy_json() {
        let err = parse_lifecycle_policy("{not-json").unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn rejects_empty_rules() {
        let err = parse_lifecycle_policy(r#"{"rules":[]}"#).unwrap_err();
        assert!(err.message.contains("at least one rule"));
    }

    #[test]
    fn rejects_tagged_without_prefix_or_pattern() {
        let policy = r#"{
            "rules": [{
                "rulePriority": 1,
                "selection": {
                    "tagStatus": "tagged",
                    "countType": "imageCountMoreThan",
                    "countNumber": 5
                },
                "action": { "type": "expire" }
            }]
        }"#;
        let err = parse_lifecycle_policy(policy).unwrap_err();
        assert!(err.message.contains("tagPrefixList"), "{err:?}");
    }

    #[test]
    fn rejects_unknown_count_type() {
        let policy = r#"{
            "rules": [{
                "rulePriority": 1,
                "selection": {
                    "tagStatus": "any",
                    "countType": "bogus",
                    "countNumber": 1
                },
                "action": { "type": "expire" }
            }]
        }"#;
        let err = parse_lifecycle_policy(policy).unwrap_err();
        assert!(err.message.contains("countType"), "{err:?}");
    }

    #[test]
    fn rules_sorted_by_priority() {
        let policy = r#"{
            "rules": [
                { "rulePriority": 10, "selection": { "tagStatus": "untagged", "countType": "imageCountMoreThan", "countNumber": 5 }, "action": { "type": "expire" } },
                { "rulePriority": 1,  "selection": { "tagStatus": "untagged", "countType": "imageCountMoreThan", "countNumber": 3 }, "action": { "type": "expire" } }
            ]
        }"#;
        let parsed = parse_lifecycle_policy(policy).unwrap();
        assert_eq!(parsed.rules[0].priority, 1);
        assert_eq!(parsed.rules[1].priority, 10);
    }

    #[test]
    fn count_more_than_keeps_newest_n() {
        let policy = parse_lifecycle_policy(
            r#"{
                "rules": [{
                    "rulePriority": 1,
                    "description": "keep 2 untagged",
                    "selection": {
                        "tagStatus": "untagged",
                        "countType": "imageCountMoreThan",
                        "countNumber": 2
                    },
                    "action": { "type": "expire" }
                }]
            }"#,
        )
        .unwrap();
        let images = vec![
            image("sha:1", None, 100),
            image("sha:2", None, 200),
            image("sha:3", None, 300),
            image("sha:4", None, 400),
        ];
        let expired = evaluate_lifecycle_policy(&policy, &images, 1_000);
        let digests: Vec<_> = expired.iter().map(|(d, _, _)| d.as_str()).collect();
        assert!(digests.contains(&"sha:1"));
        assert!(digests.contains(&"sha:2"));
        assert!(!digests.contains(&"sha:3"));
        assert!(!digests.contains(&"sha:4"));
    }

    #[test]
    fn since_image_pushed_expires_old_images() {
        let policy = parse_lifecycle_policy(
            r#"{
                "rules": [{
                    "rulePriority": 1,
                    "selection": {
                        "tagStatus": "any",
                        "countType": "sinceImagePushed",
                        "countUnit": "days",
                        "countNumber": 1
                    },
                    "action": { "type": "expire" }
                }]
            }"#,
        )
        .unwrap();
        let now = 2 * 86_400;
        let images = vec![image("old", None, 0), image("recent", None, now - 60)];
        let expired = evaluate_lifecycle_policy(&policy, &images, now);
        let digests: Vec<_> = expired.iter().map(|(d, _, _)| d.as_str()).collect();
        assert_eq!(digests, vec!["old"]);
    }

    #[test]
    fn tag_prefix_list_selects_matching_tags() {
        let policy = parse_lifecycle_policy(
            r#"{
                "rules": [{
                    "rulePriority": 1,
                    "selection": {
                        "tagStatus": "tagged",
                        "tagPrefixList": ["dev-"],
                        "countType": "imageCountMoreThan",
                        "countNumber": 0
                    },
                    "action": { "type": "expire" }
                }]
            }"#,
        )
        .unwrap();
        let images = vec![
            image("a", Some("dev-old"), 100),
            image("b", Some("prod"), 200),
        ];
        let expired = evaluate_lifecycle_policy(&policy, &images, 1_000);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "a");
    }

    #[test]
    fn pattern_matches_glob_with_star() {
        assert!(pattern_matches("v1.*", "v1.0"));
        assert!(pattern_matches("v1.*", "v1.99"));
        assert!(!pattern_matches("v1.*", "v2.0"));
        assert!(pattern_matches("*-dev", "feature-dev"));
        assert!(pattern_matches("rel-*-prod", "rel-2024-prod"));
        assert!(!pattern_matches("v1.*", "production-v1.0"));
    }
}
