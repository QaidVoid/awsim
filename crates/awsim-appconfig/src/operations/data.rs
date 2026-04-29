use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{AppConfigState, ConfigurationSession};

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

/// Resolve "name or id" to an actual resource id by checking both fields.
fn resolve_app(state: &AppConfigState, ident: &str) -> Option<String> {
    if state.applications.contains_key(ident) {
        return Some(ident.to_string());
    }
    state
        .applications
        .iter()
        .find(|e| e.value().name == ident)
        .map(|e| e.value().id.clone())
}

fn resolve_env(state: &AppConfigState, app_id: &str, ident: &str) -> Option<String> {
    if state
        .environments
        .contains_key(&format!("{app_id}:{ident}"))
    {
        return Some(ident.to_string());
    }
    state
        .environments
        .iter()
        .find(|e| {
            let env = e.value();
            env.application_id == app_id && env.name == ident
        })
        .map(|e| e.value().id.clone())
}

fn resolve_profile(state: &AppConfigState, app_id: &str, ident: &str) -> Option<String> {
    if state.profiles.contains_key(&format!("{app_id}:{ident}")) {
        return Some(ident.to_string());
    }
    state
        .profiles
        .iter()
        .find(|e| {
            let p = e.value();
            p.application_id == app_id && p.name == ident
        })
        .map(|e| e.value().id.clone())
}

pub fn start_configuration_session(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let app_ident = require_str(input, "ApplicationIdentifier")?;
    let env_ident = require_str(input, "EnvironmentIdentifier")?;
    let prof_ident = require_str(input, "ConfigurationProfileIdentifier")?;
    let token = uuid::Uuid::new_v4().to_string();
    let session = ConfigurationSession {
        token: token.clone(),
        application_identifier: app_ident.to_string(),
        environment_identifier: env_ident.to_string(),
        configuration_profile_identifier: prof_ident.to_string(),
        last_version_label: None,
    };
    state.sessions.insert(token.clone(), session);
    Ok(json!({ "InitialConfigurationToken": token }))
}

pub fn get_latest_configuration(
    state: &AppConfigState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

    let token = require_str(input, "ConfigurationToken")?.to_string();
    let session = state
        .sessions
        .get(&token)
        .ok_or_else(|| AwsError::bad_request("BadRequestException", "Invalid ConfigurationToken"))?
        .clone();

    let app_id = resolve_app(state, &session.application_identifier).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Application {} not found", session.application_identifier),
        )
    })?;
    let _env_id =
        resolve_env(state, &app_id, &session.environment_identifier).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Environment {} not found", session.environment_identifier),
            )
        })?;
    let profile_id = resolve_profile(state, &app_id, &session.configuration_profile_identifier)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!(
                    "Profile {} not found",
                    session.configuration_profile_identifier
                ),
            )
        })?;

    // Find the latest hosted version for this profile.
    let latest = state
        .hosted_versions
        .iter()
        .filter(|e| {
            let h = e.value();
            h.application_id == app_id && h.configuration_profile_id == profile_id
        })
        .max_by_key(|e| e.value().version_number)
        .map(|e| e.value().clone());

    // Roll forward the session token; the AWS SDK stores it for the next poll.
    let next_token = uuid::Uuid::new_v4().to_string();
    let mut new_session = session.clone();
    new_session.token = next_token.clone();
    if let Some(h) = &latest {
        new_session.last_version_label = h
            .version_label
            .clone()
            .or_else(|| Some(h.version_number.to_string()));
    }
    state.sessions.remove(&token);
    state.sessions.insert(next_token.clone(), new_session);

    match latest {
        Some(h) => Ok(json!({
            "NextPollConfigurationToken": next_token,
            "NextPollIntervalInSeconds": 60,
            "ContentType": h.content_type,
            "VersionLabel": h.version_label.clone().unwrap_or_else(|| h.version_number.to_string()),
            "Configuration": B64.encode(&h.content),
        })),
        None => Ok(json!({
            "NextPollConfigurationToken": next_token,
            "NextPollIntervalInSeconds": 60,
            "ContentType": "application/json",
            "VersionLabel": "",
            "Configuration": "",
        })),
    }
}
