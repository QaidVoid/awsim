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
    prune_hosted_versions(state, &app_id, &pid);
    Ok(result)
}

/// AWS AppConfig keeps at most 100 hosted versions per
/// `(ApplicationId, ConfigurationProfileId)`. After a successful
/// create, drop the oldest entries until the count is back inside the
/// cap so the next create never trips the limit.
const HOSTED_VERSION_CAP: usize = 100;

fn prune_hosted_versions(state: &AppConfigState, app_id: &str, profile_id: &str) {
    let mut versions: Vec<u32> = state
        .hosted_versions
        .iter()
        .filter(|e| {
            let h = e.value();
            h.application_id == app_id && h.configuration_profile_id == profile_id
        })
        .map(|e| e.value().version_number)
        .collect();
    if versions.len() <= HOSTED_VERSION_CAP {
        return;
    }
    versions.sort();
    let to_drop = versions.len() - HOSTED_VERSION_CAP;
    for v in versions.into_iter().take(to_drop) {
        state
            .hosted_versions
            .remove(&hosted_key(app_id, profile_id, v));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::applications::create_application;
    use crate::operations::profiles::create_profile;

    fn ctx() -> RequestContext {
        RequestContext::new("appconfig", "us-east-1")
    }

    fn setup() -> (AppConfigState, String, String) {
        let state = AppConfigState::default();
        let app = create_application(&state, &json!({ "Name": "app" }), &ctx()).unwrap();
        let app_id = app["Id"].as_str().unwrap().to_string();
        let prof = create_profile(
            &state,
            &json!({
                "ApplicationId": app_id,
                "Name": "p",
                "LocationUri": "hosted",
            }),
            &ctx(),
        )
        .unwrap();
        let profile_id = prof["Id"].as_str().unwrap().to_string();
        (state, app_id, profile_id)
    }

    #[test]
    fn hosted_versions_pruned_at_100_cap() {
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
        let (state, app_id, profile_id) = setup();
        for _ in 0..101 {
            create_hosted_version(
                &state,
                &json!({
                    "ApplicationId": app_id,
                    "ConfigurationProfileId": profile_id,
                    "Content": B64.encode(b"{}"),
                    "ContentType": "application/json",
                }),
                &ctx(),
            )
            .unwrap();
        }
        // Only the 100 most recent versions survive.
        let kept: Vec<u32> = state
            .hosted_versions
            .iter()
            .filter(|e| {
                let h = e.value();
                h.application_id == app_id && h.configuration_profile_id == profile_id
            })
            .map(|e| e.value().version_number)
            .collect();
        assert_eq!(kept.len(), 100);
        let min = kept.iter().copied().min().unwrap();
        let max = kept.iter().copied().max().unwrap();
        assert_eq!(max, 101);
        assert_eq!(min, 2, "oldest (version 1) should have been evicted");
    }
}
