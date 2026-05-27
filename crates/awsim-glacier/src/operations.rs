use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Archive, GlacierState, Job, Vault, archive_key, job_key};

fn iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

fn ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mins = secs / 60;
    let mi = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let mut days = hours / 24;
    let mut y = 1970u64;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let months: &[u64] = if is_leap(y) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut rem = days;
    let mut mo = 1u64;
    for &dm in months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }
    (y, mo, rem + 1, h, mi, s)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        AwsError::bad_request(
            "MissingParameterValueException",
            format!("{key} is required"),
        )
    })
}

/// Resolve the path-style `accountId` against the caller. Glacier's
/// REST paths embed an account id, and AWS lets callers pass `-` as a
/// stand-in for "my own account". Anything else has to match the
/// signing account exactly; a mismatched id is rejected with
/// `AccessDeniedException`, matching real Glacier. Absent / `-`
/// silently maps to `ctx.account_id`.
fn resolve_account_id(input: &Value, ctx: &RequestContext) -> Result<(), AwsError> {
    match input.get("accountId").and_then(Value::as_str) {
        None | Some("-") => Ok(()),
        Some(id) if id == ctx.account_id => Ok(()),
        Some(other) => Err(AwsError::forbidden(
            "AccessDeniedException",
            format!(
                "Account id `{other}` in the path does not match the signing account `{}`.",
                ctx.account_id
            ),
        )),
    }
}

fn vault_arn(ctx: &RequestContext, vault: &str) -> String {
    format!(
        "arn:aws:glacier:{}:{}:vaults/{}",
        ctx.region, ctx.account_id, vault
    )
}

fn vault_to_value(v: &Vault) -> Value {
    json!({
        "VaultName": v.vault_name,
        "VaultARN": v.vault_arn,
        "CreationDate": v.creation_date,
        "LastInventoryDate": v.last_inventory_date,
        "NumberOfArchives": v.number_of_archives,
        "SizeInBytes": v.size_in_bytes,
    })
}

fn job_to_value(j: &Job) -> Value {
    json!({
        "VaultARN": j.vault_name,
        "JobId": j.job_id,
        "Action": j.action,
        "ArchiveId": j.archive_id,
        "StatusCode": j.status_code,
        "CreationDate": j.creation_date,
        "CompletionDate": j.completion_date,
        "StatusMessage": j.status_message,
        "JobDescription": j.job_description,
        "SNSTopic": j.sns_topic,
        "Tier": j.tier,
        "Completed": j.status_code == "Succeeded",
    })
}

pub fn create_vault(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?.to_string();
    if state.vaults.contains_key(&vault) {
        // Glacier returns 201 + Location header on either fresh or pre-existing
        // vaults — emulator collapses to ok.
        return Ok(json!({ "Location": format!("/-/vaults/{vault}") }));
    }
    let v = Vault {
        vault_name: vault.clone(),
        vault_arn: vault_arn(ctx, &vault),
        creation_date: iso_now(),
        last_inventory_date: None,
        number_of_archives: 0,
        size_in_bytes: 0,
        notification_topic: None,
        notification_events: vec![],
    };
    state.vaults.insert(vault.clone(), v);
    Ok(json!({ "Location": format!("/-/vaults/{vault}") }))
}

pub fn describe_vault(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let v = state.vaults.get(vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    Ok(vault_to_value(&v))
}

pub fn list_vaults(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let items: Vec<Value> = state
        .vaults
        .iter()
        .map(|e| vault_to_value(e.value()))
        .collect();
    Ok(json!({ "VaultList": items, "Marker": null }))
}

pub fn delete_vault(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let v = state.vaults.get(vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    if v.number_of_archives > 0 {
        return Err(AwsError::bad_request(
            "InvalidParameterValueException",
            "Vault still contains archives",
        ));
    }
    drop(v);
    state.vaults.remove(vault);
    Ok(json!({}))
}

pub fn upload_archive(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use sha2::{Digest, Sha256};
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?.to_string();
    let _ = state.vaults.get(&vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    let body_b64 = input.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let body = if body_b64.is_empty() {
        Vec::new()
    } else {
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
        B64.decode(body_b64).unwrap_or_default()
    };
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let hash = format!("{:x}", hasher.finalize());

    let archive_id = uuid::Uuid::new_v4().simple().to_string();
    let archive = Archive {
        vault_name: vault.clone(),
        archive_id: archive_id.clone(),
        creation_date: iso_now(),
        size: body.len() as u64,
        sha256_tree_hash: hash.clone(),
        description: input
            .get("archiveDescription")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    state
        .archives
        .insert(archive_key(&vault, &archive_id), archive);
    if let Some(mut v) = state.vaults.get_mut(&vault) {
        v.number_of_archives += 1;
        v.size_in_bytes += body.len() as u64;
    }
    Ok(json!({
        "Location": format!("/-/vaults/{vault}/archives/{archive_id}"),
        "Checksum": hash,
        "ArchiveId": archive_id,
    }))
}

pub fn delete_archive(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?.to_string();
    let archive_id = require_str(input, "archiveId")?.to_string();
    let (_, a) = state
        .archives
        .remove(&archive_key(&vault, &archive_id))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Archive {archive_id} not found"),
            )
        })?;
    if let Some(mut v) = state.vaults.get_mut(&vault) {
        if v.number_of_archives > 0 {
            v.number_of_archives -= 1;
        }
        v.size_in_bytes = v.size_in_bytes.saturating_sub(a.size);
    }
    Ok(json!({}))
}

pub fn initiate_job(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?.to_string();
    let _ = state.vaults.get(&vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    let params = input
        .get("jobParameters")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let action = params
        .get("Type")
        .and_then(|v| v.as_str())
        .unwrap_or("inventory-retrieval")
        .to_string();
    let archive_id = params
        .get("ArchiveId")
        .and_then(|v| v.as_str())
        .map(String::from);
    let job_id = uuid::Uuid::new_v4().simple().to_string();
    let now = iso_now();
    // Emulator collapses queued/running — jobs land in Succeeded immediately.
    let job = Job {
        vault_name: vault.clone(),
        job_id: job_id.clone(),
        action,
        archive_id,
        status_code: "Succeeded".to_string(),
        creation_date: now.clone(),
        completion_date: Some(now),
        status_message: None,
        job_description: params
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        sns_topic: params
            .get("SNSTopic")
            .and_then(|v| v.as_str())
            .map(String::from),
        tier: params
            .get("Tier")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    state.jobs.insert(job_key(&vault, &job_id), job);
    Ok(json!({
        "Location": format!("/-/vaults/{vault}/jobs/{job_id}"),
        "JobId": job_id,
    }))
}

pub fn describe_job(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let job_id = require_str(input, "jobId")?;
    let j = state.jobs.get(&job_key(vault, job_id)).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Job {job_id} not found"),
        )
    })?;
    Ok(job_to_value(&j))
}

pub fn list_jobs(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let items: Vec<Value> = state
        .jobs
        .iter()
        .filter(|e| e.value().vault_name == vault)
        .map(|e| job_to_value(e.value()))
        .collect();
    Ok(json!({ "JobList": items, "Marker": null }))
}

pub fn set_vault_notifications(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let mut v = state.vaults.get_mut(vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    let cfg = input.get("vaultNotificationConfig");
    v.notification_topic = cfg
        .and_then(|c| c.get("SNSTopic"))
        .and_then(|t| t.as_str())
        .map(String::from);
    v.notification_events = cfg
        .and_then(|c| c.get("Events"))
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({}))
}

pub fn get_vault_notifications(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let v = state.vaults.get(vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    Ok(json!({
        "vaultNotificationConfig": {
            "SNSTopic": v.notification_topic,
            "Events": v.notification_events,
        }
    }))
}

pub fn delete_vault_notifications(
    state: &GlacierState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    resolve_account_id(input, ctx)?;
    let vault = require_str(input, "vaultName")?;
    let mut v = state.vaults.get_mut(vault).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Vault {vault} not found"),
        )
    })?;
    v.notification_topic = None;
    v.notification_events.clear();
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("glacier", "us-east-1")
    }

    fn state_with_vault(name: &str) -> GlacierState {
        let state = GlacierState::default();
        create_vault(&state, &json!({ "vaultName": name }), &ctx()).unwrap();
        state
    }

    #[test]
    fn dash_account_id_is_honored() {
        let state = state_with_vault("v1");
        // A `-` in the path should be treated as the caller's account.
        let v = describe_vault(
            &state,
            &json!({ "accountId": "-", "vaultName": "v1" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(v["VaultName"], "v1");
    }

    #[test]
    fn absent_account_id_falls_back_to_caller() {
        let state = state_with_vault("v1");
        let v = describe_vault(&state, &json!({ "vaultName": "v1" }), &ctx()).unwrap();
        assert_eq!(v["VaultName"], "v1");
    }

    #[test]
    fn matching_account_id_is_accepted() {
        let state = state_with_vault("v1");
        let c = ctx();
        let v = describe_vault(
            &state,
            &json!({ "accountId": c.account_id.clone(), "vaultName": "v1" }),
            &c,
        )
        .unwrap();
        assert_eq!(v["VaultName"], "v1");
    }

    #[test]
    fn mismatched_account_id_is_rejected() {
        let state = state_with_vault("v1");
        let err = describe_vault(
            &state,
            &json!({ "accountId": "999999999999", "vaultName": "v1" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccessDeniedException");
    }
}
