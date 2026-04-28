use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{CognitoState, RiskConfiguration};

// ---------------------------------------------------------------------------
// SetRiskConfiguration
// ---------------------------------------------------------------------------

pub fn set_risk_configuration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"].as_str().map(String::from);

    let config = RiskConfiguration {
        client_id: client_id.clone(),
        compromised_credentials_config: if input["CompromisedCredentialsRiskConfiguration"]
            .is_null()
        {
            None
        } else {
            Some(input["CompromisedCredentialsRiskConfiguration"].clone())
        },
        account_takeover_config: if input["AccountTakeoverRiskConfiguration"].is_null() {
            None
        } else {
            Some(input["AccountTakeoverRiskConfiguration"].clone())
        },
        risk_exception_config: if input["RiskExceptionConfiguration"].is_null() {
            None
        } else {
            Some(input["RiskExceptionConfiguration"].clone())
        },
    };

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    // Replace if exists for same client_id key
    let key = client_id.as_deref().unwrap_or("pool");
    pool.risk_configurations
        .retain(|c| c.client_id.as_deref().unwrap_or("pool") != key);
    pool.risk_configurations.push(config.clone());

    info!(pool_id = %pool_id, "Cognito: set risk configuration");
    Ok(json!({
        "RiskConfiguration": {
            "UserPoolId": pool_id,
            "ClientId": config.client_id,
            "CompromisedCredentialsRiskConfiguration": config.compromised_credentials_config,
            "AccountTakeoverRiskConfiguration": config.account_takeover_config,
            "RiskExceptionConfiguration": config.risk_exception_config
        }
    }))
}

// ---------------------------------------------------------------------------
// DescribeRiskConfiguration
// ---------------------------------------------------------------------------

pub fn describe_risk_configuration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"].as_str();

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let key = client_id.unwrap_or("pool");
    let config = pool
        .risk_configurations
        .iter()
        .find(|c| c.client_id.as_deref().unwrap_or("pool") == key);

    if let Some(c) = config {
        Ok(json!({
            "RiskConfiguration": {
                "UserPoolId": pool_id,
                "ClientId": c.client_id,
                "CompromisedCredentialsRiskConfiguration": c.compromised_credentials_config,
                "AccountTakeoverRiskConfiguration": c.account_takeover_config,
                "RiskExceptionConfiguration": c.risk_exception_config
            }
        }))
    } else {
        Ok(json!({
            "RiskConfiguration": {
                "UserPoolId": pool_id,
                "ClientId": client_id
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// UpdateAuthEventFeedback
// ---------------------------------------------------------------------------

pub fn update_auth_event_feedback(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let event_id = input["EventId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EventId is required"))?;
    // FeedbackToken is opaque from our side; presence is required but we
    // don't validate it.
    let _feedback_token = input["FeedbackToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "FeedbackToken is required"))?;
    let feedback_value = input["FeedbackValue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "FeedbackValue is required"))?;
    apply_feedback(state, pool_id, username, event_id, feedback_value, ctx)
}

// ---------------------------------------------------------------------------
// AdminUpdateAuthEventFeedback
// ---------------------------------------------------------------------------

pub fn admin_update_auth_event_feedback(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let event_id = input["EventId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EventId is required"))?;
    let feedback_value = input["FeedbackValue"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "FeedbackValue is required"))?;
    apply_feedback(state, pool_id, username, event_id, feedback_value, ctx)
}

fn apply_feedback(
    state: &CognitoState,
    pool_id: &str,
    username: &str,
    event_id: &str,
    feedback: &str,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    let event = user
        .auth_events
        .iter_mut()
        .find(|e| e.event_id == event_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Auth event not found: {event_id}"),
            )
        })?;
    event.feedback_value = Some(feedback.to_string());
    info!(username = %username, event_id = %event_id, "Cognito: auth event feedback recorded");
    Ok(json!({}))
}
