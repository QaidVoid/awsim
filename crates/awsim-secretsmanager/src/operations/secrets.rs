use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::error;
use crate::state::{Secret, SecretVersion, SecretsState};
use crate::util::{
    new_version_id, now_epoch_f64, random_password, random_suffix, validate_client_request_token,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a SecretId which may be a name or an ARN. Returns the canonical name key.
fn resolve_name(state: &SecretsState, secret_id: &str) -> Result<String, AwsError> {
    // Direct name lookup
    if state.secrets.contains_key(secret_id) {
        return Ok(secret_id.to_string());
    }

    // ARN lookup — ARN format: arn:aws:secretsmanager:{region}:{account}:secret:{name}-{suffix}
    if secret_id.starts_with("arn:aws:secretsmanager:") {
        for entry in state.secrets.iter() {
            if entry.value().arn == secret_id {
                return Ok(entry.key().clone());
            }
        }
        return Err(error::resource_not_found(secret_id));
    }

    Err(error::resource_not_found(secret_id))
}

fn build_arn(region: &str, account_id: &str, name: &str) -> String {
    let suffix = random_suffix(6);
    format!("arn:aws:secretsmanager:{region}:{account_id}:secret:{name}-{suffix}")
}

fn secret_metadata(secret: &Secret) -> Value {
    let versions_to_stages: serde_json::Map<String, Value> = secret
        .versions
        .iter()
        .map(|(vid, v)| {
            let stages: Vec<Value> = v.stages.iter().map(|s| json!(s)).collect();
            (vid.clone(), json!(stages))
        })
        .collect();

    let mut meta = json!({
        "ARN": secret.arn,
        "Name": secret.name,
        "Description": secret.description,
        "CreatedDate": secret.created_date,
        "LastChangedDate": secret.last_changed_date,
        "VersionIdsToStages": versions_to_stages,
        "RotationEnabled": secret.rotation_enabled,
    });

    if let Some(ref arn) = secret.rotation_lambda_arn {
        meta["RotationLambdaARN"] = json!(arn);
    }
    if let Some(days) = secret.rotation_automatically_after_days {
        meta["RotationRules"] = json!({ "AutomaticallyAfterDays": days });
    }
    if let Some(ref kms) = secret.kms_key_id {
        meta["KmsKeyId"] = json!(kms);
    }
    if let Some(ts) = secret.last_rotated_date {
        meta["LastRotatedDate"] = json!(ts);
    }
    if let Some(ts) = secret.last_accessed_date {
        meta["LastAccessedDate"] = json!(ts);
    }

    if !secret.tags.is_empty() {
        let tags: Vec<Value> = secret
            .tags
            .iter()
            .map(|(k, v)| json!({ "Key": k, "Value": v }))
            .collect();
        meta["Tags"] = json!(tags);
    }

    if let Some(ref dd) = secret.deleted_date {
        meta["DeletedDate"] = json!(dd);
    }

    meta
}

// ---------------------------------------------------------------------------
// CreateSecret
// ---------------------------------------------------------------------------

pub fn create_secret(
    state: &SecretsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("Name"))?;
    validate_secret_name(name)?;

    if state.secrets.contains_key(name) {
        return Err(error::resource_exists(name));
    }

    let description = input["Description"].as_str().unwrap_or("").to_string();

    let secret_string = input["SecretString"].as_str().map(|s| s.to_string());
    let secret_binary = input["SecretBinary"].as_str().map(|s| s.to_string());

    if secret_string.is_none() && secret_binary.is_none() {
        return Err(error::invalid_parameter(
            "Either SecretString or SecretBinary must be provided",
        ));
    }

    // Tags
    let mut tags = HashMap::new();
    if let Some(tag_list) = input["Tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    let arn = build_arn(&ctx.region, &ctx.account_id, name);
    let now = now_epoch_f64();
    // ClientRequestToken doubles as the VersionId (idempotency key) when
    // supplied; the SDK auto-generates one client-side otherwise. We
    // accept the caller's choice and only fall back when absent.
    let version_id = match input["ClientRequestToken"].as_str() {
        Some(t) => validate_client_request_token(t)?,
        None => new_version_id(),
    };

    let version = SecretVersion {
        version_id: version_id.clone(),
        secret_string,
        secret_binary,
        stages: vec!["AWSCURRENT".to_string()],
        created_date: now,
    };

    let mut versions = HashMap::new();
    versions.insert(version_id.clone(), version);

    let replica_regions = parse_replica_regions(&input["AddReplicaRegions"], &ctx.region)?;

    let secret = Secret {
        arn: arn.clone(),
        name: name.to_string(),
        description,
        versions,
        current_version_id: version_id.clone(),
        tags,
        created_date: now,
        last_changed_date: now,
        deleted_date: None,
        rotation_enabled: false,
        rotation_lambda_arn: None,
        rotation_automatically_after_days: None,
        kms_key_id: input["KmsKeyId"].as_str().map(str::to_string),
        last_rotated_date: None,
        last_accessed_date: None,
        replica_regions: replica_regions.clone(),
    };

    info!(name = %name, arn = %arn, "Created secret");
    state.secrets.insert(name.to_string(), secret);

    let mut response = json!({
        "ARN": arn,
        "Name": name,
        "VersionId": version_id,
    });
    if !replica_regions.is_empty() {
        response["ReplicationStatus"] =
            serde_json::Value::Array(replica_regions.iter().map(replica_status_value).collect());
    }
    Ok(response)
}

/// Parse the `AddReplicaRegions` array, rejecting entries that duplicate
/// or target the primary region. AWS surfaces both via
/// InvalidParameterException with distinct messages — we collapse to the
/// same code, which is what the SDK already keys off.
fn parse_replica_regions(
    value: &Value,
    primary_region: &str,
) -> Result<Vec<crate::state::ReplicaRegion>, AwsError> {
    let Some(arr) = value.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in arr {
        let region = entry
            .get("Region")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                error::invalid_parameter("AddReplicaRegions entry requires non-empty Region.")
            })?;
        if region == primary_region {
            return Err(error::invalid_parameter(format!(
                "AddReplicaRegions cannot include the primary region `{region}`."
            )));
        }
        if !seen.insert(region.to_string()) {
            return Err(error::invalid_parameter(format!(
                "AddReplicaRegions duplicates region `{region}`."
            )));
        }
        let kms_key_id = entry
            .get("KmsKeyId")
            .and_then(Value::as_str)
            .map(str::to_string);
        out.push(crate::state::ReplicaRegion {
            region: region.to_string(),
            kms_key_id,
        });
    }
    Ok(out)
}

fn replica_status_value(r: &crate::state::ReplicaRegion) -> Value {
    let mut obj = json!({
        "Region": r.region,
        "Status": "InSync",
    });
    if let Some(ref k) = r.kms_key_id {
        obj["KmsKeyId"] = json!(k);
    }
    obj
}

// ---------------------------------------------------------------------------
// GetSecretValue
// ---------------------------------------------------------------------------

pub fn get_secret_value(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_some() {
        return Err(error::invalid_request("Secret is marked for deletion"));
    }

    // Drop the read guard and re-acquire mutably to stamp LastAccessedDate.
    drop(secret);
    if let Some(mut s) = state.secrets.get_mut(&name) {
        s.last_accessed_date = Some(now_epoch_f64());
    }
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let requested_stage = input["VersionStage"].as_str();
    let requested_version_id = input["VersionId"].as_str();

    let version_id = match (requested_version_id, requested_stage) {
        // Both supplied: AWS requires that the named version actually
        // carries the named stage, otherwise it's a mismatch and the
        // service returns ResourceNotFoundException rather than
        // silently honouring one and dropping the other.
        (Some(vid), Some(stage)) => {
            let v = secret
                .versions
                .get(vid)
                .ok_or_else(|| error::resource_not_found(vid))?;
            if !v.stages.iter().any(|s| s == stage) {
                return Err(error::resource_not_found(&format!(
                    "version {vid} does not carry stage {stage}"
                )));
            }
            vid.to_string()
        }
        (Some(vid), None) => {
            if !secret.versions.contains_key(vid) {
                return Err(error::resource_not_found(vid));
            }
            vid.to_string()
        }
        (None, stage_or_default) => {
            let stage = stage_or_default.unwrap_or("AWSCURRENT");
            secret
                .versions
                .iter()
                .find(|(_, v)| v.stages.iter().any(|s| s == stage))
                .map(|(id, _)| id.clone())
                .ok_or_else(|| error::resource_not_found(&format!("stage {stage}")))?
        }
    };

    let version = secret
        .versions
        .get(&version_id)
        .ok_or_else(|| error::resource_not_found(&version_id))?;

    let mut response = json!({
        "ARN": secret.arn,
        "Name": secret.name,
        "VersionId": version.version_id,
        "VersionStages": version.stages,
        "CreatedDate": version.created_date,
    });

    if let Some(ref ss) = version.secret_string {
        response["SecretString"] = json!(ss);
    }
    if let Some(ref sb) = version.secret_binary {
        response["SecretBinary"] = json!(sb);
    }

    Ok(response)
}

// ---------------------------------------------------------------------------
// PutSecretValue
// ---------------------------------------------------------------------------

pub fn put_secret_value(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_some() {
        return Err(error::invalid_request("Secret is marked for deletion"));
    }

    let secret_string = input["SecretString"].as_str().map(|s| s.to_string());
    let secret_binary = input["SecretBinary"].as_str().map(|s| s.to_string());

    if secret_string.is_none() && secret_binary.is_none() {
        return Err(error::invalid_parameter(
            "Either SecretString or SecretBinary must be provided",
        ));
    }

    let now = now_epoch_f64();
    let client_token = match input["ClientRequestToken"].as_str() {
        Some(t) => Some(validate_client_request_token(t)?),
        None => None,
    };

    // Idempotency: if the same ClientRequestToken already exists on this
    // secret, AWS returns the existing version when the payload matches
    // and ResourceExistsException when it doesn't.
    if let Some(ref token) = client_token
        && let Some(existing) = secret.versions.get(token)
    {
        let payload_matches =
            existing.secret_string == secret_string && existing.secret_binary == secret_binary;
        if !payload_matches {
            return Err(error::resource_exists(token));
        }
        let arn = secret.arn.clone();
        let sname = secret.name.clone();
        let stages = existing.stages.clone();
        let vid = existing.version_id.clone();
        drop(secret);
        return Ok(json!({
            "ARN": arn,
            "Name": sname,
            "VersionId": vid,
            "VersionStages": stages,
        }));
    }

    let new_version_id_str = client_token.unwrap_or_else(new_version_id);

    // Determine stages for new version. AWS treats both an omitted
    // VersionStages field and an empty array the same way: the new
    // version is tagged AWSCURRENT (and the prior AWSCURRENT shifts to
    // AWSPREVIOUS below).
    let requested_stages: Vec<String> = match input["VersionStages"].as_array() {
        Some(stages) if !stages.is_empty() => stages
            .iter()
            .filter_map(|s| s.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec!["AWSCURRENT".to_string()],
    };

    // If new version will be AWSCURRENT, demote old AWSCURRENT to AWSPREVIOUS
    if requested_stages.contains(&"AWSCURRENT".to_string()) {
        let old_current = secret.current_version_id.clone();
        if let Some(old_ver) = secret.versions.get_mut(&old_current) {
            old_ver.stages.retain(|s| s != "AWSCURRENT");
            if !old_ver.stages.contains(&"AWSPREVIOUS".to_string()) {
                old_ver.stages.push("AWSPREVIOUS".to_string());
            }
        }
        secret.current_version_id = new_version_id_str.clone();
    }

    let new_version = SecretVersion {
        version_id: new_version_id_str.clone(),
        secret_string,
        secret_binary,
        stages: requested_stages.clone(),
        created_date: now,
    };

    secret
        .versions
        .insert(new_version_id_str.clone(), new_version);
    secret.last_changed_date = now;

    let arn = secret.arn.clone();
    let sname = secret.name.clone();
    drop(secret);

    Ok(json!({
        "ARN": arn,
        "Name": sname,
        "VersionId": new_version_id_str,
        "VersionStages": requested_stages,
    }))
}

// ---------------------------------------------------------------------------
// DescribeSecret
// ---------------------------------------------------------------------------

pub fn describe_secret(
    state: &SecretsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let mut meta = secret_metadata(&secret);
    meta["OwnerAccountId"] = json!(ctx.account_id);
    meta["PrimaryRegion"] = json!(ctx.region);
    meta["ReplicationStatus"] = Value::Array(
        secret
            .replica_regions
            .iter()
            .map(replica_status_value)
            .collect(),
    );
    Ok(meta)
}

// ---------------------------------------------------------------------------
// ListSecrets
// ---------------------------------------------------------------------------

pub fn list_secrets(
    state: &SecretsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filters = parse_list_filters(input)?;
    let include_planned_deletion = input["IncludePlannedDeletion"].as_bool().unwrap_or(false);

    let mut secrets: Vec<Secret> = state
        .secrets
        .iter()
        .filter(|entry| {
            let s = entry.value();
            if !include_planned_deletion && s.deleted_date.is_some() {
                return false;
            }
            filters.iter().all(|f| f.matches(s, &ctx.region))
        })
        .map(|entry| entry.value().clone())
        .collect();

    // SortOrder operates on the secret's CreatedDate per AWS docs.
    let sort_order = input["SortOrder"].as_str().unwrap_or("asc");
    secrets.sort_by(|a, b| match sort_order {
        "desc" => b
            .created_date
            .partial_cmp(&a.created_date)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => a
            .created_date
            .partial_cmp(&b.created_date)
            .unwrap_or(std::cmp::Ordering::Equal),
    });

    let list: Vec<Value> = secrets.iter().map(secret_metadata).collect();
    Ok(json!({ "SecretList": list }))
}

/// A single Filter entry from the ListSecrets request.
struct ListFilter {
    key: String,
    values: Vec<String>,
}

impl ListFilter {
    fn matches(&self, s: &Secret, region: &str) -> bool {
        // AWS treats multiple values within a single filter as OR; they
        // also do prefix matching on string fields and accept a leading
        // `!` to negate.
        self.values.iter().any(|raw| {
            let (negate, needle) = match raw.strip_prefix('!') {
                Some(stripped) => (true, stripped),
                None => (false, raw.as_str()),
            };
            let hit = match self.key.as_str() {
                "name" => s.name.contains(needle),
                "description" => s.description.contains(needle),
                "tag-key" => s.tags.keys().any(|k| k.contains(needle)),
                "tag-value" => s.tags.values().any(|v| v.contains(needle)),
                // Secrets live in a per-region store, so every secret
                // visible here is primary in `region`.
                "primary-region" => region == needle,
                // No cross-account ownership in awsim, so every secret
                // is owned by the calling account.
                "owned-by-me" => needle.eq_ignore_ascii_case("true"),
                "owning-service" => false,
                "all" => {
                    s.name.contains(needle)
                        || s.description.contains(needle)
                        || s.tags.keys().any(|k| k.contains(needle))
                        || s.tags.values().any(|v| v.contains(needle))
                }
                _ => false,
            };
            if negate { !hit } else { hit }
        })
    }
}

fn parse_list_filters(input: &Value) -> Result<Vec<ListFilter>, AwsError> {
    let Some(arr) = input["Filters"].as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(arr.len());
    for f in arr {
        let key = f["Key"].as_str().ok_or_else(|| {
            error::invalid_parameter("Filter.Key is required and must be a string")
        })?;
        let values: Vec<String> = f["Values"]
            .as_array()
            .map(|vs| {
                vs.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(ListFilter {
            key: key.to_string(),
            values,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// UpdateSecret
// ---------------------------------------------------------------------------

pub fn update_secret(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    // AWS UpdateSecret deliberately does not accept rotation-related
    // fields; those go through RotateSecret / CancelRotateSecret.
    // Reject early so callers can't sneak rotation changes through the
    // update path.
    for field in ["RotationLambdaARN", "RotationRules", "RotationEnabled"] {
        if !input[field].is_null() {
            return Err(error::invalid_request(format!(
                "UpdateSecret does not accept {field}; use RotateSecret or CancelRotateSecret."
            )));
        }
    }

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_some() {
        return Err(error::invalid_request("Secret is marked for deletion"));
    }

    if let Some(desc) = input["Description"].as_str() {
        secret.description = desc.to_string();
    }
    if let Some(kms) = input["KmsKeyId"].as_str() {
        secret.kms_key_id = Some(kms.to_string());
    }

    let has_new_value =
        input["SecretString"].as_str().is_some() || input["SecretBinary"].as_str().is_some();

    let now = now_epoch_f64();

    if has_new_value {
        let secret_string = input["SecretString"].as_str().map(|s| s.to_string());
        let secret_binary = input["SecretBinary"].as_str().map(|s| s.to_string());
        let new_vid = new_version_id();

        // Demote old AWSCURRENT
        let old_current = secret.current_version_id.clone();
        if let Some(old_ver) = secret.versions.get_mut(&old_current) {
            old_ver.stages.retain(|s| s != "AWSCURRENT");
            if !old_ver.stages.contains(&"AWSPREVIOUS".to_string()) {
                old_ver.stages.push("AWSPREVIOUS".to_string());
            }
        }

        let new_version = SecretVersion {
            version_id: new_vid.clone(),
            secret_string,
            secret_binary,
            stages: vec!["AWSCURRENT".to_string()],
            created_date: now,
        };
        secret.versions.insert(new_vid.clone(), new_version);
        secret.current_version_id = new_vid;
    }

    secret.last_changed_date = now;

    let arn = secret.arn.clone();
    let sname = secret.name.clone();
    let vid = secret.current_version_id.clone();
    drop(secret);

    Ok(json!({
        "ARN": arn,
        "Name": sname,
        "VersionId": vid,
    }))
}

// ---------------------------------------------------------------------------
// DeleteSecret
// ---------------------------------------------------------------------------

pub fn delete_secret(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_some() {
        return Err(error::invalid_request(
            "Secret is already scheduled for deletion",
        ));
    }

    let force = input["ForceDeleteWithoutRecovery"]
        .as_bool()
        .unwrap_or(false);

    let arn = secret.arn.clone();
    let sname = secret.name.clone();

    if force {
        drop(secret);
        state.secrets.remove(&name);
        return Ok(json!({
            "ARN": arn,
            "Name": sname,
            "DeletionDate": now_epoch_f64(),
        }));
    }

    let recovery_days = input["RecoveryWindowInDays"].as_u64().unwrap_or(30);
    if !(7..=30).contains(&recovery_days) {
        return Err(error::invalid_parameter(
            "RecoveryWindowInDays must be between 7 and 30",
        ));
    }

    use std::time::{SystemTime, UNIX_EPOCH};
    let deletion_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        + (recovery_days * 86400) as f64;

    secret.deleted_date = Some(deletion_epoch);
    drop(secret);

    info!(name = %name, "Secret scheduled for deletion");

    Ok(json!({
        "ARN": arn,
        "Name": sname,
        "DeletionDate": deletion_epoch,
    }))
}

// ---------------------------------------------------------------------------
// RestoreSecret
// ---------------------------------------------------------------------------

pub fn restore_secret(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_none() {
        return Err(error::invalid_request(
            "Secret is not scheduled for deletion",
        ));
    }

    secret.deleted_date = None;

    let arn = secret.arn.clone();
    let sname = secret.name.clone();
    drop(secret);

    Ok(json!({ "ARN": arn, "Name": sname }))
}

// ---------------------------------------------------------------------------
// TagResource
// ---------------------------------------------------------------------------

pub fn tag_resource(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    awsim_core::tags::validate_aws_tags(&input["Tags"], &awsim_core::tags::TagOpts::aws_default())?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if let Some(tag_list) = input["Tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                secret.tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UntagResource
// ---------------------------------------------------------------------------

pub fn untag_resource(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    awsim_core::tags::validate_aws_tag_keys(&input["TagKeys"])?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if let Some(key_list) = input["TagKeys"].as_array() {
        for key in key_list {
            if let Some(k) = key.as_str() {
                secret.tags.remove(k);
            }
        }
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RotateSecret
// ---------------------------------------------------------------------------

pub fn rotate_secret(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    if secret.deleted_date.is_some() {
        return Err(error::invalid_request("Secret is marked for deletion"));
    }

    // Store rotation configuration if provided
    if let Some(lambda_arn) = input["RotationLambdaARN"].as_str() {
        secret.rotation_lambda_arn = Some(lambda_arn.to_string());
    }
    if let Some(rules) = input["RotationRules"].as_object()
        && let Some(days) = rules.get("AutomaticallyAfterDays").and_then(|v| v.as_u64())
    {
        secret.rotation_automatically_after_days = Some(days);
    }
    secret.rotation_enabled = true;

    // Simulate rotation: create a new AWSPENDING version then immediately promote it to AWSCURRENT.
    let now = now_epoch_f64();
    let pending_vid = new_version_id();

    // Clone the current value into the new version (no real Lambda invocation)
    let current_value = secret
        .versions
        .get(&secret.current_version_id)
        .map(|v| (v.secret_string.clone(), v.secret_binary.clone()));
    let (secret_string, secret_binary) = current_value.unwrap_or((None, None));

    // Mark old AWSCURRENT as AWSPREVIOUS
    let old_current_id = secret.current_version_id.clone();
    if let Some(old_ver) = secret.versions.get_mut(&old_current_id) {
        old_ver.stages.retain(|s| s != "AWSCURRENT");
        if !old_ver.stages.contains(&"AWSPREVIOUS".to_string()) {
            old_ver.stages.push("AWSPREVIOUS".to_string());
        }
    }

    let new_version = SecretVersion {
        version_id: pending_vid.clone(),
        secret_string,
        secret_binary,
        stages: vec!["AWSCURRENT".to_string()],
        created_date: now,
    };
    secret.versions.insert(pending_vid.clone(), new_version);
    secret.current_version_id = pending_vid.clone();
    secret.last_changed_date = now;
    secret.last_rotated_date = Some(now);

    let arn = secret.arn.clone();
    let sname = secret.name.clone();
    drop(secret);

    info!(name = %name, "RotateSecret (stub)");

    Ok(json!({
        "ARN": arn,
        "Name": sname,
        "VersionId": pending_vid,
    }))
}

// ---------------------------------------------------------------------------
// CancelRotateSecret
// ---------------------------------------------------------------------------

pub fn cancel_rotate_secret(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    secret.rotation_enabled = false;
    secret.rotation_lambda_arn = None;

    let arn = secret.arn.clone();
    let sname = secret.name.clone();
    let vid = secret.current_version_id.clone();
    drop(secret);

    Ok(json!({
        "ARN": arn,
        "Name": sname,
        "VersionId": vid,
    }))
}

// ---------------------------------------------------------------------------
// ValidateResourcePolicy
// ---------------------------------------------------------------------------

pub fn validate_resource_policy(
    _state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let policy = input["ResourcePolicy"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("ResourcePolicy"))?;
    let issues = check_policy_structure(policy);
    let validation_errors: Vec<Value> = issues
        .iter()
        .map(|m| json!({ "CheckName": "ValidateResourcePolicy", "ErrorMessage": m }))
        .collect();
    Ok(json!({
        "PolicyValidationPassed": validation_errors.is_empty(),
        "ValidationErrors": validation_errors,
    }))
}

/// Cheap structural validation of an IAM-shaped resource policy. Flags
/// missing fields and wrong shapes; doesn't attempt full IAM semantic
/// validation. Returns a list of human-readable issues (empty == valid).
fn check_policy_structure(policy: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let parsed: Value = match serde_json::from_str(policy) {
        Ok(v) => v,
        Err(e) => {
            issues.push(format!("Policy is not valid JSON: {e}"));
            return issues;
        }
    };
    let statements = match parsed.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(_)) => vec![parsed["Statement"].clone()],
        Some(_) => {
            issues.push("Statement must be an object or array of objects".to_string());
            return issues;
        }
        None => {
            issues.push("Policy is missing a Statement".to_string());
            return issues;
        }
    };
    for (i, stmt) in statements.iter().enumerate() {
        let prefix = format!("Statement[{i}]");
        match stmt.get("Effect").and_then(|v| v.as_str()) {
            Some("Allow") | Some("Deny") => {}
            Some(other) => issues.push(format!(
                "{prefix}.Effect must be Allow or Deny, got {other}"
            )),
            None => issues.push(format!("{prefix} is missing Effect")),
        }
        if stmt.get("Action").is_none() && stmt.get("NotAction").is_none() {
            issues.push(format!("{prefix} must specify Action or NotAction"));
        }
        if stmt.get("Principal").is_none() && stmt.get("NotPrincipal").is_none() {
            issues.push(format!("{prefix} must specify Principal or NotPrincipal"));
        }
        if stmt.get("Resource").is_none() && stmt.get("NotResource").is_none() {
            issues.push(format!("{prefix} must specify Resource or NotResource"));
        }
    }
    issues
}

/// Returns true when any Allow statement names a wildcard Principal — i.e.
/// `Principal: "*"` or `Principal.AWS: "*"`. AWS uses this signal for
/// BlockPublicPolicy on PutResourcePolicy.
fn policy_grants_public_access(policy: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(policy) else {
        return false;
    };
    let statements: Vec<Value> = match parsed.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(_)) => vec![parsed["Statement"].clone()],
        _ => return false,
    };
    fn principal_is_wildcard(p: &Value) -> bool {
        match p {
            Value::String(s) => s == "*",
            Value::Array(a) => a.iter().any(principal_is_wildcard),
            Value::Object(o) => o.values().any(principal_is_wildcard),
            _ => false,
        }
    }
    statements.iter().any(|s| {
        s.get("Effect").and_then(|v| v.as_str()) == Some("Allow")
            && s.get("Principal").is_some_and(principal_is_wildcard)
    })
}

// ---------------------------------------------------------------------------
// GetRandomPassword
// ---------------------------------------------------------------------------

pub fn get_random_password(
    _state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let length = input["PasswordLength"].as_u64().unwrap_or(32) as usize;
    if !(1..=4096).contains(&length) {
        return Err(error::invalid_parameter(
            "PasswordLength must be between 1 and 4096",
        ));
    }

    let exclude_uppercase = input["ExcludeUppercase"].as_bool().unwrap_or(false);
    let exclude_lowercase = input["ExcludeLowercase"].as_bool().unwrap_or(false);
    let exclude_numbers = input["ExcludeNumbers"].as_bool().unwrap_or(false);
    let exclude_punctuation = input["ExcludePunctuation"].as_bool().unwrap_or(false);

    let password = random_password(
        length,
        exclude_uppercase,
        exclude_lowercase,
        exclude_numbers,
        exclude_punctuation,
    );

    Ok(json!({ "RandomPassword": password }))
}

// ---------------------------------------------------------------------------
// ReplicateSecretToRegions (stub)
// ---------------------------------------------------------------------------

pub fn replicate_secret_to_regions(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let arn = secret.arn.clone();
    drop(secret);

    Ok(json!({
        "ARN": arn,
        "ReplicationStatus": [],
    }))
}

// ---------------------------------------------------------------------------
// RemoveRegionsFromReplication (stub)
// ---------------------------------------------------------------------------

pub fn remove_regions_from_replication(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let arn = secret.arn.clone();
    drop(secret);

    Ok(json!({
        "ARN": arn,
        "ReplicationStatus": [],
    }))
}

// ---------------------------------------------------------------------------
// StopReplicationToReplica (stub)
// ---------------------------------------------------------------------------

pub fn stop_replication_to_replica(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let arn = secret.arn.clone();
    drop(secret);

    Ok(json!({ "ARN": arn }))
}

// ---------------------------------------------------------------------------
// ListSecretVersionIds
// ---------------------------------------------------------------------------

pub fn list_secret_version_ids(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let include_deprecated = input["IncludeDeprecated"].as_bool().unwrap_or(false);

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;

    let versions: Vec<Value> = secret
        .versions
        .iter()
        .filter(|(_, v)| include_deprecated || !v.stages.is_empty())
        .map(|(vid, v)| {
            let stages: Vec<Value> = v.stages.iter().map(|s| json!(s)).collect();
            let mut entry = json!({
                "VersionId": vid,
                "VersionStages": stages,
                "CreatedDate": v.created_date,
            });
            if let Some(ts) = secret.last_accessed_date {
                entry["LastAccessedDate"] = json!(ts);
            }
            if let Some(ts) = secret.last_rotated_date {
                entry["LastRotatedDate"] = json!(ts);
            }
            entry
        })
        .collect();

    Ok(json!({
        "ARN": secret.arn,
        "Name": secret.name,
        "Versions": versions,
        "Truncated": false,
    }))
}

// ---------------------------------------------------------------------------
// BatchGetSecretValue
// ---------------------------------------------------------------------------

pub fn batch_get_secret_value(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id_list = input["SecretIdList"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut secret_values: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for id_val in &secret_id_list {
        let secret_id = match id_val.as_str() {
            Some(s) => s,
            None => continue,
        };

        match resolve_name(state, secret_id) {
            Ok(name) => {
                let secret = match state.secrets.get(&name) {
                    Some(s) => s,
                    None => {
                        errors.push(json!({
                            "SecretId": secret_id,
                            "ErrorCode": "ResourceNotFoundException",
                            "Message": format!("Secrets Manager can't find the specified secret: {secret_id}"),
                        }));
                        continue;
                    }
                };

                if secret.deleted_date.is_some() {
                    errors.push(json!({
                        "SecretId": secret_id,
                        "ErrorCode": "InvalidRequestException",
                        "Message": "Secret is marked for deletion",
                    }));
                    continue;
                }

                let version = match secret.versions.get(&secret.current_version_id) {
                    Some(v) => v,
                    None => {
                        errors.push(json!({
                            "SecretId": secret_id,
                            "ErrorCode": "ResourceNotFoundException",
                            "Message": "No current version found",
                        }));
                        continue;
                    }
                };

                let mut entry = json!({
                    "ARN": secret.arn,
                    "Name": secret.name,
                    "VersionId": version.version_id,
                    "VersionStages": version.stages,
                    "CreatedDate": version.created_date,
                });
                if let Some(ref ss) = version.secret_string {
                    entry["SecretString"] = json!(ss);
                }
                if let Some(ref sb) = version.secret_binary {
                    entry["SecretBinary"] = json!(sb);
                }
                secret_values.push(entry);
            }
            Err(_) => {
                errors.push(json!({
                    "SecretId": secret_id,
                    "ErrorCode": "ResourceNotFoundException",
                    "Message": format!("Secrets Manager can't find the specified secret: {secret_id}"),
                }));
            }
        }
    }

    Ok(json!({
        "SecretValues": secret_values,
        "Errors": errors,
    }))
}

// ---------------------------------------------------------------------------
// UpdateSecretVersionStage
// ---------------------------------------------------------------------------

pub fn update_secret_version_stage(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;
    let version_stage = input["VersionStage"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("VersionStage"))?;
    let remove_from = input["RemoveFromVersionId"].as_str();
    let move_to = input["MoveToVersionId"].as_str();

    let name = resolve_name(state, secret_id)?;
    let mut secret = state
        .secrets
        .get_mut(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;
    let arn = secret.arn.clone();
    let secret_name = secret.name.clone();

    if let Some(remove_id) = remove_from
        && let Some(v) = secret.versions.get_mut(remove_id)
    {
        v.stages.retain(|s| s != version_stage);
    }

    if let Some(move_id) = move_to {
        if !secret.versions.contains_key(move_id) {
            return Err(error::resource_not_found(move_id));
        }
        for (vid, v) in secret.versions.iter_mut() {
            if vid != move_id {
                v.stages.retain(|s| s != version_stage);
            }
        }
        if let Some(v) = secret.versions.get_mut(move_id)
            && !v.stages.contains(&version_stage.to_string())
        {
            v.stages.push(version_stage.to_string());
        }
        if version_stage == "AWSCURRENT" {
            secret.current_version_id = move_id.to_string();
        }
    }

    secret.last_changed_date = now_epoch_f64();

    Ok(json!({
        "ARN": arn,
        "Name": secret_name,
    }))
}

// ---------------------------------------------------------------------------
// PutResourcePolicy
// ---------------------------------------------------------------------------

pub fn put_resource_policy(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;
    let policy = input["ResourcePolicy"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("ResourcePolicy"))?;
    let block_public = input["BlockPublicPolicy"].as_bool().unwrap_or(false);

    // Reject malformed JSON regardless of BlockPublicPolicy — AWS doesn't
    // store policies it can't parse.
    if serde_json::from_str::<Value>(policy).is_err() {
        return Err(AwsError::bad_request(
            "MalformedPolicyDocumentException",
            "ResourcePolicy is not valid JSON",
        ));
    }
    if block_public && policy_grants_public_access(policy) {
        return Err(AwsError::bad_request(
            "PublicPolicyException",
            "ResourcePolicy grants public access; pass BlockPublicPolicy=false to override",
        ));
    }

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;
    let arn = secret.arn.clone();
    let secret_name = secret.name.clone();
    drop(secret);

    state.resource_policies.insert(name, policy.to_string());

    Ok(json!({
        "ARN": arn,
        "Name": secret_name,
    }))
}

// ---------------------------------------------------------------------------
// GetResourcePolicy
// ---------------------------------------------------------------------------

pub fn get_resource_policy(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;
    let arn = secret.arn.clone();
    let secret_name = secret.name.clone();
    drop(secret);

    let policy = state
        .resource_policies
        .get(&name)
        .map(|e| e.value().clone());

    let mut response = json!({
        "ARN": arn,
        "Name": secret_name,
    });
    if let Some(p) = policy {
        response["ResourcePolicy"] = json!(p);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// DeleteResourcePolicy
// ---------------------------------------------------------------------------

pub fn delete_resource_policy(
    state: &SecretsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let secret_id = input["SecretId"]
        .as_str()
        .ok_or_else(|| error::missing_parameter("SecretId"))?;

    let name = resolve_name(state, secret_id)?;
    let secret = state
        .secrets
        .get(&name)
        .ok_or_else(|| error::resource_not_found(secret_id))?;
    let arn = secret.arn.clone();
    let secret_name = secret.name.clone();
    drop(secret);

    state.resource_policies.remove(&name);

    Ok(json!({
        "ARN": arn,
        "Name": secret_name,
    }))
}

/// Validate a secret name against AWS rules:
///   - 1..=512 characters
///   - charset: alphanumerics plus `/_+=.@-` (the `/` is for path-like
///     names used by some SDKs / reserved-prefix detection below)
///   - the `aws/` prefix is reserved for AWS-managed secrets and rejected
///     for customer creates with InvalidRequestException
fn validate_secret_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 512 {
        return Err(error::invalid_parameter(format!(
            "Secret name length {} is outside 1..=512",
            name.len()
        )));
    }
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '=' | '.' | '@' | '-'));
    if !valid_chars {
        return Err(error::invalid_parameter(
            "Secret names may only contain alphanumeric characters and the chars /_+=.@-",
        ));
    }
    if name.starts_with("aws/") {
        return Err(AwsError::bad_request(
            "InvalidRequestException",
            "Secret names may not start with the reserved prefix 'aws/'",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("secretsmanager", "us-east-1")
    }

    fn token(prefix: &str) -> String {
        // 32-char minimum: pad with letters.
        format!("{prefix:x<32}")
    }

    #[test]
    fn create_secret_uses_client_request_token_as_version_id() {
        let state = SecretsState::default();
        let tok = token("a");
        let resp = create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "hello",
                "ClientRequestToken": tok,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["VersionId"].as_str().unwrap(), tok);
    }

    #[test]
    fn create_secret_rejects_short_client_request_token() {
        let state = SecretsState::default();
        let err = create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "hello",
                "ClientRequestToken": "tooshort",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn put_secret_value_returns_existing_version_for_idempotent_replay() {
        let state = SecretsState::default();
        create_secret(
            &state,
            &json!({ "Name": "s", "SecretString": "v1" }),
            &ctx(),
        )
        .unwrap();
        let tok = token("b");

        let first = put_secret_value(
            &state,
            &json!({
                "SecretId": "s",
                "SecretString": "v2",
                "ClientRequestToken": tok,
            }),
            &ctx(),
        )
        .unwrap();
        let replay = put_secret_value(
            &state,
            &json!({
                "SecretId": "s",
                "SecretString": "v2",
                "ClientRequestToken": tok,
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(first["VersionId"], replay["VersionId"]);
        assert_eq!(replay["VersionId"].as_str().unwrap(), tok);
    }

    #[test]
    fn list_secrets_filters_owned_by_me_and_primary_region() {
        let state = SecretsState::default();
        create_secret(&state, &json!({ "Name": "a", "SecretString": "v" }), &ctx()).unwrap();
        create_secret(&state, &json!({ "Name": "b", "SecretString": "v" }), &ctx()).unwrap();

        // owned-by-me=true matches everything; false matches nothing.
        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "owned-by-me", "Values": ["true"] }] }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["SecretList"].as_array().unwrap().len(), 2);
        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "owned-by-me", "Values": ["false"] }] }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["SecretList"].as_array().unwrap().is_empty());

        // primary-region matches when value equals ctx.region (us-east-1).
        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "primary-region", "Values": ["us-east-1"] }] }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["SecretList"].as_array().unwrap().len(), 2);
        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "primary-region", "Values": ["us-west-2"] }] }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["SecretList"].as_array().unwrap().is_empty());
    }

    #[test]
    fn put_secret_value_empty_stages_defaults_to_awscurrent() {
        let state = SecretsState::default();
        create_secret(
            &state,
            &json!({ "Name": "s", "SecretString": "v1" }),
            &ctx(),
        )
        .unwrap();

        let resp = put_secret_value(
            &state,
            &json!({
                "SecretId": "s",
                "SecretString": "v2",
                "VersionStages": [],
            }),
            &ctx(),
        )
        .unwrap();
        let stages = resp["VersionStages"].as_array().unwrap();
        assert!(stages.iter().any(|s| s == "AWSCURRENT"));

        let secret = state.secrets.get("s").unwrap();
        let prev = secret
            .versions
            .values()
            .find(|v| v.stages.iter().any(|s| s == "AWSPREVIOUS"))
            .expect("previous AWSCURRENT should have moved to AWSPREVIOUS");
        assert_eq!(prev.secret_string.as_deref(), Some("v1"));
    }

    #[test]
    fn create_secret_persists_kms_key_id_in_describe() {
        let state = SecretsState::default();
        create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "v",
                "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
            }),
            &ctx(),
        )
        .unwrap();
        let resp = describe_secret(&state, &json!({ "SecretId": "s" }), &ctx()).unwrap();
        assert_eq!(
            resp["KmsKeyId"],
            json!("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
    }

    #[test]
    fn rotate_secret_stamps_last_rotated_date() {
        let state = SecretsState::default();
        create_secret(&state, &json!({ "Name": "s", "SecretString": "v" }), &ctx()).unwrap();
        let before = describe_secret(&state, &json!({ "SecretId": "s" }), &ctx()).unwrap();
        assert!(before.get("LastRotatedDate").is_none());

        rotate_secret(&state, &json!({ "SecretId": "s" }), &ctx()).unwrap();

        let after = describe_secret(&state, &json!({ "SecretId": "s" }), &ctx()).unwrap();
        assert!(after.get("LastRotatedDate").is_some());
    }

    #[test]
    fn validate_resource_policy_flags_missing_principal() {
        let state = SecretsState::default();
        let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"secretsmanager:GetSecretValue","Resource":"*"}]}"#;
        let resp =
            validate_resource_policy(&state, &json!({ "ResourcePolicy": policy }), &ctx()).unwrap();
        assert_eq!(resp["PolicyValidationPassed"], json!(false));
        let errors = resp["ValidationErrors"].as_array().unwrap();
        assert!(!errors.is_empty());
        let combined: String = errors
            .iter()
            .filter_map(|e| e["ErrorMessage"].as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(combined.to_lowercase().contains("principal"));
    }

    #[test]
    fn validate_resource_policy_passes_complete_policy() {
        let state = SecretsState::default();
        let policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": { "AWS": "arn:aws:iam::000000000000:root" },
                "Action": "secretsmanager:GetSecretValue",
                "Resource": "*"
            }]
        }"#;
        let resp =
            validate_resource_policy(&state, &json!({ "ResourcePolicy": policy }), &ctx()).unwrap();
        assert_eq!(resp["PolicyValidationPassed"], json!(true));
    }

    #[test]
    fn put_resource_policy_rejects_public_policy_when_block_set() {
        let state = SecretsState::default();
        create_secret(&state, &json!({ "Name": "s", "SecretString": "v" }), &ctx()).unwrap();
        let public_policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": "*",
                "Action": "secretsmanager:GetSecretValue",
                "Resource": "*"
            }]
        }"#;
        let err = put_resource_policy(
            &state,
            &json!({
                "SecretId": "s",
                "ResourcePolicy": public_policy,
                "BlockPublicPolicy": true,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "PublicPolicyException");

        // Same policy without BlockPublicPolicy succeeds.
        put_resource_policy(
            &state,
            &json!({
                "SecretId": "s",
                "ResourcePolicy": public_policy,
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn put_resource_policy_rejects_malformed_json() {
        let state = SecretsState::default();
        create_secret(&state, &json!({ "Name": "s", "SecretString": "v" }), &ctx()).unwrap();
        let err = put_resource_policy(
            &state,
            &json!({
                "SecretId": "s",
                "ResourcePolicy": "{ not json",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MalformedPolicyDocumentException");
    }

    #[test]
    fn list_secrets_filters_by_name_and_negates() {
        let state = SecretsState::default();
        create_secret(
            &state,
            &json!({ "Name": "alpha", "SecretString": "v" }),
            &ctx(),
        )
        .unwrap();
        create_secret(
            &state,
            &json!({ "Name": "beta", "SecretString": "v" }),
            &ctx(),
        )
        .unwrap();

        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "name", "Values": ["alpha"] }] }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<&str> = resp["SecretList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["alpha"]);

        let resp = list_secrets(
            &state,
            &json!({ "Filters": [{ "Key": "name", "Values": ["!alpha"] }] }),
            &ctx(),
        )
        .unwrap();
        let names: Vec<&str> = resp["SecretList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["beta"]);
    }

    #[test]
    fn put_secret_value_rejects_token_reuse_with_different_payload() {
        let state = SecretsState::default();
        create_secret(
            &state,
            &json!({ "Name": "s", "SecretString": "v1" }),
            &ctx(),
        )
        .unwrap();
        let tok = token("c");
        put_secret_value(
            &state,
            &json!({
                "SecretId": "s",
                "SecretString": "v2",
                "ClientRequestToken": tok,
            }),
            &ctx(),
        )
        .unwrap();

        let err = put_secret_value(
            &state,
            &json!({
                "SecretId": "s",
                "SecretString": "different-payload",
                "ClientRequestToken": tok,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ResourceExistsException");
    }

    #[test]
    fn create_secret_persists_add_replica_regions() {
        let state = SecretsState::default();
        let resp = create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "v",
                "AddReplicaRegions": [
                    { "Region": "us-west-2" },
                    { "Region": "eu-west-1", "KmsKeyId": "alias/replica" }
                ],
            }),
            &ctx(),
        )
        .unwrap();
        let status = resp["ReplicationStatus"]
            .as_array()
            .expect("ReplicationStatus");
        assert_eq!(status.len(), 2);
        assert_eq!(status[1]["KmsKeyId"], "alias/replica");

        let desc = describe_secret(&state, &json!({ "SecretId": "s" }), &ctx()).unwrap();
        let dstatus = desc["ReplicationStatus"].as_array().unwrap();
        assert_eq!(dstatus.len(), 2);
    }

    #[test]
    fn create_secret_rejects_replica_in_primary_region() {
        let state = SecretsState::default();
        let err = create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "v",
                "AddReplicaRegions": [{ "Region": "us-east-1" }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("primary"));
    }

    #[test]
    fn create_secret_rejects_duplicate_replica_region() {
        let state = SecretsState::default();
        let err = create_secret(
            &state,
            &json!({
                "Name": "s",
                "SecretString": "v",
                "AddReplicaRegions": [
                    { "Region": "us-west-2" },
                    { "Region": "us-west-2" }
                ],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("duplicates"));
    }
}
