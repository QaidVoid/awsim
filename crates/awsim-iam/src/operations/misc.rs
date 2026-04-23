/// Miscellaneous IAM operations that are stubs or return empty data.
use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::{
    error::no_such_entity,
    ids::{new_access_key_id, new_secret_access_key, now_iso8601},
    state::{IamState, ServiceSpecificCredential, SigningCertificate},
};

use super::{opt_str, require_str};

// ── ListServiceSpecificCredentials ────────────────────────────────────────────

/// ListServiceSpecificCredentials — Return credentials owned by user.
pub fn list_service_specific_credentials(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let user_name = input.get("UserName").and_then(|v| v.as_str());
    if let Some(name) = user_name {
        if !state.users.contains_key(name) {
            return Err(crate::error::no_such_entity("User", name));
        }
    }
    let service_filter = opt_str(input, "ServiceName");

    let creds: Vec<Value> = state
        .service_specific_credentials
        .iter()
        .filter(|c| {
            user_name.map(|u| c.value().user_name == u).unwrap_or(true)
                && service_filter
                    .map(|s| c.value().service_name == s)
                    .unwrap_or(true)
        })
        .map(|c| {
            json!({
                "UserName": c.value().user_name,
                "Status": c.value().status,
                "ServiceUserName": c.value().service_user_name,
                "CreateDate": c.value().create_date,
                "ServiceSpecificCredentialId": c.value().service_specific_credential_id,
                "ServiceName": c.value().service_name,
            })
        })
        .collect();

    Ok(json!({
        "ServiceSpecificCredentials": { "member": creds }
    }))
}

// ── ListSigningCertificates ───────────────────────────────────────────────────

/// ListSigningCertificates — Return certificates owned by the user.
pub fn list_signing_certificates(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let user_name = input.get("UserName").and_then(|v| v.as_str());
    if let Some(name) = user_name {
        if !state.users.contains_key(name) {
            return Err(crate::error::no_such_entity("User", name));
        }
    }

    let certs: Vec<Value> = state
        .signing_certificates
        .iter()
        .filter(|c| user_name.map(|u| c.value().user_name == u).unwrap_or(true))
        .map(|c| {
            json!({
                "UserName": c.value().user_name,
                "CertificateId": c.value().certificate_id,
                "CertificateBody": c.value().certificate_body,
                "Status": c.value().status,
                "UploadDate": c.value().upload_date,
            })
        })
        .collect();

    Ok(json!({
        "Certificates": { "member": certs },
        "IsTruncated": false
    }))
}

pub fn upload_signing_certificate(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let body = require_str(input, "CertificateBody")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    let id = format!("ASCA{}", new_access_key_id().trim_start_matches("AKIA"));
    let cert = SigningCertificate {
        user_name: user_name.to_string(),
        certificate_id: id.clone(),
        certificate_body: body.to_string(),
        status: "Active".to_string(),
        upload_date: now_iso8601(),
    };

    let result = json!({
        "UserName": cert.user_name,
        "CertificateId": cert.certificate_id,
        "CertificateBody": cert.certificate_body,
        "Status": cert.status,
        "UploadDate": cert.upload_date,
    });
    state.signing_certificates.insert(id, cert);

    Ok(json!({ "Certificate": result }))
}

pub fn update_signing_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let cert_id = require_str(input, "CertificateId")?;
    let status = require_str(input, "Status")?;

    let mut cert = state
        .signing_certificates
        .get_mut(cert_id)
        .ok_or_else(|| no_such_entity("SigningCertificate", cert_id))?;
    cert.status = status.to_string();
    Ok(json!({}))
}

pub fn delete_signing_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let cert_id = require_str(input, "CertificateId")?;
    if state.signing_certificates.remove(cert_id).is_none() {
        return Err(no_such_entity("SigningCertificate", cert_id));
    }
    Ok(json!({}))
}

pub fn create_service_specific_credential(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let user_name = require_str(input, "UserName")?;
    let service_name = require_str(input, "ServiceName")?;

    if !state.users.contains_key(user_name) {
        return Err(no_such_entity("User", user_name));
    }

    let cred_id = new_access_key_id();
    let cred = ServiceSpecificCredential {
        user_name: user_name.to_string(),
        service_name: service_name.to_string(),
        service_user_name: format!("{user_name}-{}", &cred_id[4..8]),
        service_specific_credential_id: cred_id.clone(),
        service_password: new_secret_access_key(),
        status: "Active".to_string(),
        create_date: now_iso8601(),
    };

    let result = json!({
        "CreateDate": cred.create_date,
        "ServiceName": cred.service_name,
        "ServiceUserName": cred.service_user_name,
        "ServicePassword": cred.service_password,
        "ServiceSpecificCredentialId": cred.service_specific_credential_id,
        "UserName": cred.user_name,
        "Status": cred.status,
    });
    state.service_specific_credentials.insert(cred_id, cred);

    Ok(json!({ "ServiceSpecificCredential": result }))
}

pub fn delete_service_specific_credential(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cred_id = require_str(input, "ServiceSpecificCredentialId")?;
    if state.service_specific_credentials.remove(cred_id).is_none() {
        return Err(no_such_entity("ServiceSpecificCredential", cred_id));
    }
    Ok(json!({}))
}

pub fn reset_service_specific_credential(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cred_id = require_str(input, "ServiceSpecificCredentialId")?;

    let mut cred = state
        .service_specific_credentials
        .get_mut(cred_id)
        .ok_or_else(|| no_such_entity("ServiceSpecificCredential", cred_id))?;
    cred.service_password = new_secret_access_key();

    Ok(json!({
        "ServiceSpecificCredential": {
            "CreateDate": cred.create_date,
            "ServiceName": cred.service_name,
            "ServiceUserName": cred.service_user_name,
            "ServicePassword": cred.service_password,
            "ServiceSpecificCredentialId": cred.service_specific_credential_id,
            "UserName": cred.user_name,
            "Status": cred.status,
        }
    }))
}

pub fn update_service_specific_credential(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let cred_id = require_str(input, "ServiceSpecificCredentialId")?;
    let status = require_str(input, "Status")?;

    let mut cred = state
        .service_specific_credentials
        .get_mut(cred_id)
        .ok_or_else(|| no_such_entity("ServiceSpecificCredential", cred_id))?;
    cred.status = status.to_string();
    Ok(json!({}))
}

pub fn list_policies_granting_service_access(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let _arn = require_str(input, "Arn")?;
    let services = input
        .get("ServiceNamespaces")
        .and_then(|v| v.get("member"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let entries: Vec<Value> = services
        .iter()
        .filter_map(|s| s.as_str())
        .map(|svc| {
            json!({
                "ServiceNamespace": svc,
                "Policies": { "member": [] },
            })
        })
        .collect();
    Ok(json!({
        "PoliciesGrantingServiceAccess": { "member": entries },
        "IsTruncated": false,
    }))
}

pub fn get_service_last_accessed_details_with_entities(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let _job_id = require_str(input, "JobId")?;
    let _service_namespace = opt_str(input, "ServiceNamespace");
    Ok(json!({
        "JobStatus": "COMPLETED",
        "JobCreationDate": now_iso8601(),
        "JobCompletionDate": now_iso8601(),
        "EntityDetailsList": { "member": [] },
        "IsTruncated": false,
    }))
}

pub fn set_security_token_service_preferences(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn get_organizations_access_report(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let _job_id = require_str(input, "JobId")?;
    Ok(json!({
        "JobStatus": "COMPLETED",
        "JobCreationDate": now_iso8601(),
        "JobCompletionDate": now_iso8601(),
        "NumberOfServicesAccessible": 0u64,
        "NumberOfServicesNotAccessed": 0u64,
        "AccessDetails": { "member": [] },
        "IsTruncated": false,
    }))
}

pub fn generate_organizations_access_report(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let _entity_path = require_str(input, "EntityPath")?;
    Ok(json!({ "JobId": crate::ids::new_uuid() }))
}

// ── Policy Simulator stubs ────────────────────────────────────────────────────

/// SimulateCustomPolicy — Stub that returns "allowed" for all actions.
pub fn simulate_custom_policy(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let action_names = input
        .get("ActionNames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = action_names
        .iter()
        .filter_map(|a| a.as_str())
        .map(|action| {
            json!({
                "EvalActionName": action,
                "EvalDecision": "allowed",
                "EvalResourceName": "*",
                "MatchedStatements": { "member": [] },
                "MissingContextValues": { "member": [] }
            })
        })
        .collect();

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

/// SimulatePrincipalPolicy — Stub that returns "allowed" for all actions.
pub fn simulate_principal_policy(
    _state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let action_names = input
        .get("ActionNames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = action_names
        .iter()
        .filter_map(|a| a.as_str())
        .map(|action| {
            json!({
                "EvalActionName": action,
                "EvalDecision": "allowed",
                "EvalResourceName": "*",
                "MatchedStatements": { "member": [] },
                "MissingContextValues": { "member": [] }
            })
        })
        .collect();

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

// ── GetContextKeys stubs ──────────────────────────────────────────────────────

/// GetContextKeysForCustomPolicy — Return empty context key list.
pub fn get_context_keys_for_custom_policy(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ContextKeyNames": { "member": [] }
    }))
}

/// GetContextKeysForPrincipalPolicy — Return empty context key list.
pub fn get_context_keys_for_principal_policy(
    _state: &IamState,
    _input: &Value,
) -> Result<Value, AwsError> {
    Ok(json!({
        "ContextKeyNames": { "member": [] }
    }))
}
