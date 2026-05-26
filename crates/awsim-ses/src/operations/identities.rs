use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{EmailIdentity, SesState};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn identity_type(identity: &str) -> &'static str {
    if identity.contains('@') {
        "EMAIL_ADDRESS"
    } else {
        "DOMAIN"
    }
}

// ---------------------------------------------------------------------------
// CreateEmailIdentity
// ---------------------------------------------------------------------------

pub fn create_email_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;

    let id_type = identity_type(identity);

    let entry = EmailIdentity {
        identity: identity.to_string(),
        verified: true, // auto-verify for local dev
        identity_type: id_type.to_string(),
        created_at: now_epoch(),
        dkim_signing_attributes_origin: Some("AWS_SES".to_string()),
        dkim_signing_enabled: true,
        dkim_status: Some("SUCCESS".to_string()),
        dkim_domain_signing_selector: None,
        dkim_domain_signing_private_key: None,
        dkim_next_signing_key_length: Some("RSA_2048_BIT".to_string()),
    };

    info!(identity = %identity, "SES: created identity");
    state.identities.insert(identity.to_string(), entry);

    Ok(json!({
        "IdentityType": id_type,
        "VerifiedForSendingStatus": true,
        "DkimAttributes": {
            "SigningEnabled": true,
            "Status": "SUCCESS",
            "Tokens": []
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteEmailIdentity
// ---------------------------------------------------------------------------

pub fn delete_email_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;

    if state.identities.remove(identity).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        ));
    }

    info!(identity = %identity, "SES: deleted identity");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetEmailIdentity
// ---------------------------------------------------------------------------

pub fn get_email_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;

    let entry = state.identities.get(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;

    let mut dkim = json!({
        "SigningEnabled": entry.dkim_signing_enabled,
        "Status": entry.dkim_status.as_deref().unwrap_or("SUCCESS"),
        "Tokens": [],
        "SigningAttributesOrigin": entry
            .dkim_signing_attributes_origin
            .as_deref()
            .unwrap_or("AWS_SES"),
    });
    if let Some(ref kl) = entry.dkim_next_signing_key_length {
        dkim["NextSigningKeyLength"] = json!(kl);
    }
    if let Some(ref s) = entry.dkim_domain_signing_selector {
        dkim["CurrentSigningKeyLength"] = json!("EXTERNAL");
        dkim["DomainSigningSelector"] = json!(s);
    }
    Ok(json!({
        "IdentityType": entry.identity_type,
        "FeedbackForwardingStatus": true,
        "VerifiedForSendingStatus": entry.verified,
        "DkimAttributes": dkim,
        "MailFromAttributes": {
            "BehaviorOnMxFailure": "USE_DEFAULT_VALUE"
        },
        "Tags": []
    }))
}

// ---------------------------------------------------------------------------
// ListEmailIdentities
// ---------------------------------------------------------------------------

pub fn list_email_identities(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identities: Vec<Value> = state
        .identities
        .iter()
        .map(|e| {
            json!({
                "IdentityName": e.identity,
                "IdentityType": e.identity_type,
                "SendingEnabled": true
            })
        })
        .collect();

    Ok(json!({ "EmailIdentities": identities }))
}
