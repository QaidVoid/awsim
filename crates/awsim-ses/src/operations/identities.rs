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
        dkim_tokens: Vec::new(),
        mail_from_domain: None,
        mail_from_behavior_on_mx_failure: None,
        configuration_set_name: None,
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
    let mut mail_from = json!({
        "BehaviorOnMxFailure": entry
            .mail_from_behavior_on_mx_failure
            .as_deref()
            .unwrap_or("USE_DEFAULT_VALUE"),
    });
    if let Some(ref d) = entry.mail_from_domain {
        mail_from["MailFromDomain"] = json!(d);
        mail_from["MailFromDomainStatus"] = json!("SUCCESS");
    } else {
        mail_from["MailFromDomainStatus"] = json!("PENDING");
    }
    let tags: Vec<Value> = state
        .identity_tags
        .get(identity)
        .map(|m| {
            m.iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        })
        .unwrap_or_default();
    let policies: serde_json::Map<String, Value> = state
        .identity_policies
        .get(identity)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect()
        })
        .unwrap_or_default();
    let mut resp = json!({
        "IdentityType": entry.identity_type,
        "FeedbackForwardingStatus": true,
        "VerifiedForSendingStatus": entry.verified,
        "DkimAttributes": dkim,
        "MailFromAttributes": mail_from,
        "Policies": Value::Object(policies),
        "Tags": tags,
    });
    if let Some(ref cs) = entry.configuration_set_name {
        resp["ConfigurationSetName"] = json!(cs);
    }
    Ok(resp)
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

#[cfg(test)]
mod get_email_identity_tests {
    use super::*;
    use crate::operations::more::{
        create_configuration_set, create_email_identity_policy,
        put_email_identity_configuration_set_attributes,
        put_email_identity_dkim_signing_attributes, put_email_identity_mail_from_attributes,
    };

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn seed(state: &SesState) {
        create_email_identity(state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
    }

    #[test]
    fn returns_default_attribute_set_for_freshly_created_identity() {
        let state = SesState::default();
        seed(&state);
        let resp =
            get_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        assert_eq!(resp["IdentityType"], "DOMAIN");
        assert_eq!(resp["DkimAttributes"]["SigningEnabled"], true);
        assert_eq!(resp["DkimAttributes"]["SigningAttributesOrigin"], "AWS_SES");
        assert_eq!(
            resp["MailFromAttributes"]["MailFromDomainStatus"],
            "PENDING"
        );
        assert!(resp["Tags"].as_array().unwrap().is_empty());
        assert!(resp["Policies"].as_object().unwrap().is_empty());
        assert!(resp.get("ConfigurationSetName").is_none());
    }

    #[test]
    fn returns_mail_from_and_policies_and_configuration_set() {
        let state = SesState::default();
        seed(&state);
        create_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx()).unwrap();
        put_email_identity_mail_from_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "MailFromDomain": "bounce.example.com",
                "BehaviorOnMxFailure": "REJECT_MESSAGE",
            }),
            &ctx(),
        )
        .unwrap();
        put_email_identity_configuration_set_attributes(
            &state,
            &json!({ "EmailIdentity": "example.com", "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .unwrap();
        create_email_identity_policy(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "PolicyName": "policy-1",
                "Policy": "{\"Version\":\"2012-10-17\"}",
            }),
            &ctx(),
        )
        .unwrap();
        state
            .identity_tags
            .entry("example.com".to_string())
            .or_default()
            .insert("env".to_string(), "prod".to_string());

        let resp =
            get_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        assert_eq!(
            resp["MailFromAttributes"]["MailFromDomain"],
            "bounce.example.com"
        );
        assert_eq!(
            resp["MailFromAttributes"]["MailFromDomainStatus"],
            "SUCCESS"
        );
        assert_eq!(
            resp["MailFromAttributes"]["BehaviorOnMxFailure"],
            "REJECT_MESSAGE"
        );
        assert_eq!(resp["ConfigurationSetName"], "cs");
        assert!(resp["Policies"]["policy-1"].is_string());
        let tags = resp["Tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["Key"], "env");
    }

    #[test]
    fn surfaces_byodkim_attributes_when_set() {
        let state = SesState::default();
        seed(&state);
        put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "EXTERNAL",
                "SigningAttributes": {
                    "DomainSigningSelector": "sel",
                    "DomainSigningPrivateKey": "MIIE..."
                },
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            get_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        assert_eq!(
            resp["DkimAttributes"]["SigningAttributesOrigin"],
            "EXTERNAL"
        );
        assert_eq!(resp["DkimAttributes"]["DomainSigningSelector"], "sel");
    }

    #[test]
    fn attaching_missing_configuration_set_errors() {
        let state = SesState::default();
        seed(&state);
        let err = put_email_identity_configuration_set_attributes(
            &state,
            &json!({ "EmailIdentity": "example.com", "ConfigurationSetName": "nope" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }
}
