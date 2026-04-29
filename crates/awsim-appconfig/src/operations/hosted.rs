use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, HostedConfigVersion, hosted_key, profile_key};

fn require_app_id(input: &Value) -> Result<&str, AwsError> {
    input
        .get("ApplicationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "ApplicationId is required"))
}

fn version_to_value(h: &HostedConfigVersion) -> Value {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    json!({
        "ApplicationId": h.application_id,
        "ConfigurationProfileId": h.configuration_profile_id,
        "VersionNumber": h.version_number,
        "Description": h.description,
        "Content": B64.encode(&h.content),
        "ContentType": h.content_type,
        "VersionLabel": h.version_label,
    })
}

pub fn create_hosted_version(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

    let app_id = require_app_id(input)?.to_string();
    let pid = input
        .get("ConfigurationProfileId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "ConfigurationProfileId is required")
        })?
        .to_string();

    let mut profile = state
        .profiles
        .get_mut(&profile_key(&app_id, &pid))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Profile {pid} not found"),
            )
        })?;
    profile.latest_version_number += 1;
    let version = profile.latest_version_number;
    drop(profile);

    let content_b64 = input
        .get("Content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Content is required"))?;
    let content = B64
        .decode(content_b64)
        .map_err(|e| AwsError::bad_request("BadRequestException", format!("Bad base64: {e}")))?;

    let h = HostedConfigVersion {
        application_id: app_id.clone(),
        configuration_profile_id: pid.clone(),
        version_number: version,
        description: input
            .get("Description")
            .and_then(|v| v.as_str())
            .map(String::from),
        content,
        content_type: input
            .get("ContentType")
            .and_then(|v| v.as_str())
            .unwrap_or("application/json")
            .to_string(),
        version_label: input
            .get("VersionLabel")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let result = version_to_value(&h);
    state
        .hosted_versions
        .insert(hosted_key(&app_id, &pid, version), h);
    Ok(result)
}

pub fn get_hosted_version(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_id = require_app_id(input)?;
    let pid = input
        .get("ConfigurationProfileId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AwsError::bad_request("BadRequestException", "ConfigurationProfileId is required")
        })?;
    let version = input
        .get("VersionNumber")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "VersionNumber is required"))?
        as u32;
    let h = state
        .hosted_versions
        .get(&hosted_key(app_id, pid, version))
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Version {version} not found"),
            )
        })?;
    Ok(version_to_value(&h))
}
