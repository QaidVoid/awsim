/// Miscellaneous IAM operations that are stubs or return empty data.
use std::collections::HashMap;

use std::sync::Arc;

use awsim_core::{AuthzEngine, AwsError};
use awsim_iam_policy::{
    AuthzRequest, ContextValue, Decision, EvalContext, MatchedStatement, PolicyAttribution,
    PolicyAttributions, PolicyDocument, PolicySource, evaluate_detailed,
};
use serde_json::{Value, json};

use crate::{
    error::{malformed_policy_document, no_such_entity},
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
    if let Some(name) = user_name
        && !state.users.contains_key(name)
    {
        return Err(crate::error::no_such_entity("User", name));
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
    if let Some(name) = user_name
        && !state.users.contains_key(name)
    {
        return Err(crate::error::no_such_entity("User", name));
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

pub fn upload_signing_certificate(state: &IamState, input: &Value) -> Result<Value, AwsError> {
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

// ── Policy Simulator ─────────────────────────────────────────────────────────

fn extract_string_list(input: &Value, key: &str) -> Vec<String> {
    let v = match input.get(key) {
        Some(v) => v,
        None => return Vec::new(),
    };
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
    }
    if let Some(members) = v.get("member").and_then(|m| m.as_array()) {
        return members
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
    }
    if let Some(s) = v.as_str() {
        return vec![s.to_string()];
    }
    Vec::new()
}

fn parse_policy_input_list(input: &Value, key: &str) -> Result<Vec<PolicyDocument>, AwsError> {
    extract_string_list(input, key)
        .iter()
        .map(|raw| {
            awsim_iam_policy::parse(raw)
                .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))
        })
        .collect()
}

fn extract_context_entries(input: &Value) -> HashMap<String, ContextValue> {
    let mut ctx = HashMap::new();
    let raw = match input.get("ContextEntries") {
        Some(v) => v,
        None => return ctx,
    };
    let entries = raw
        .as_array()
        .cloned()
        .or_else(|| raw.get("member").and_then(|m| m.as_array()).cloned())
        .unwrap_or_default();
    for entry in entries {
        let key = match entry.get("ContextKeyName").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let typ = entry
            .get("ContextKeyType")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let values = extract_string_list(&entry, "ContextKeyValues");
        let value = match typ {
            "string" => ContextValue::String(values.into_iter().next().unwrap_or_default()),
            "stringList" => ContextValue::StringList(values),
            "numeric" => values
                .into_iter()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .map(ContextValue::Number)
                .unwrap_or(ContextValue::Number(0.0)),
            "boolean" => values
                .into_iter()
                .next()
                .map(|v| ContextValue::Bool(v == "true"))
                .unwrap_or(ContextValue::Bool(false)),
            "ip" => ContextValue::Ip(values.into_iter().next().unwrap_or_default()),
            _ => ContextValue::StringList(values),
        };
        ctx.insert(key, value);
    }
    ctx
}

fn decision_to_str(d: Decision) -> &'static str {
    match d {
        Decision::Allow => "allowed",
        Decision::ExplicitDeny => "explicitDeny",
        Decision::ImplicitDeny => "implicitDeny",
    }
}

/// Bundle of policies attributed to their source, plus parallel
/// attribution metadata so the simulator can report
/// `MatchedStatements` with meaningful `SourcePolicyId`s.
struct AttributedPolicies {
    identity: Vec<PolicyDocument>,
    identity_attributions: Vec<PolicyAttribution>,
    permissions_boundary: Option<PolicyDocument>,
    permissions_boundary_attribution: Option<PolicyAttribution>,
    scps: Vec<PolicyDocument>,
    scp_attributions: Vec<PolicyAttribution>,
}

fn build_evaluation_results(
    bundle: &AttributedPolicies,
    actions: &[String],
    resources: &[String],
    principal_arn: &str,
    principal_account: &str,
    context: &HashMap<String, ContextValue>,
    authz: Option<&Arc<AuthzEngine>>,
) -> Vec<Value> {
    let mut out = Vec::new();
    for action in actions {
        for resource in resources {
            // Resource policy lookups happen per (resource, action) so
            // we re-fetch on each iteration. Cheap — service lookups
            // are HashMap reads behind an Arc.
            let (resource_policy_doc, resource_attr) =
                lookup_resource_policy(authz, action, resource);

            let req = AuthzRequest {
                principal_arn,
                principal_account,
                action,
                resource_arn: resource,
                context,
            };
            let ctx = EvalContext {
                identity_policies: &bundle.identity,
                permissions_boundary: bundle.permissions_boundary.as_ref(),
                resource_policy: resource_policy_doc.as_ref(),
                scps: &bundle.scps,
                session_policy: None,
            };
            let attrs = PolicyAttributions {
                identity: &bundle.identity_attributions,
                permissions_boundary: bundle.permissions_boundary_attribution.as_ref(),
                resource: resource_attr.as_ref(),
                scps: &bundle.scp_attributions,
                session: None,
            };
            let details = evaluate_detailed(&req, &ctx, &attrs);

            out.push(json!({
                "EvalActionName": action,
                "EvalResourceName": resource,
                "EvalDecision": decision_to_str(details.decision),
                "MatchedStatements": {
                    "member": details.matched_statements.iter().map(matched_statement_to_xml).collect::<Vec<_>>()
                },
                "MissingContextValues": {
                    "member": details.missing_context_values.clone()
                }
            }));
        }
    }
    out
}

fn matched_statement_to_xml(m: &MatchedStatement) -> Value {
    json!({
        "SourcePolicyId": m.source_id,
        "SourcePolicyType": policy_source_to_str(&m.source_type),
        "StartPosition": { "Line": m.statement_index + 1, "Column": 1 },
        "EndPosition": { "Line": m.statement_index + 1, "Column": 1 },
    })
}

fn policy_source_to_str(s: &PolicySource) -> &'static str {
    match s {
        PolicySource::Identity => "IAM Policy",
        PolicySource::PermissionsBoundary => "Permissions Boundary",
        PolicySource::Resource => "Resource",
        PolicySource::Scp => "Organizations Service Control Policy",
        PolicySource::Session => "Session Policy",
    }
}

/// Pull the resource policy that would gate `action` on `resource_arn`
/// from the gateway's authz lookups. Returns `(doc, attribution)`
/// when a service owns the ARN's prefix and has a policy for it;
/// otherwise `(None, None)`.
fn lookup_resource_policy(
    authz: Option<&Arc<AuthzEngine>>,
    action: &str,
    resource_arn: &str,
) -> (Option<PolicyDocument>, Option<PolicyAttribution>) {
    let Some(authz) = authz else {
        return (None, None);
    };
    // Service prefix is the part before ':' in the action (e.g.
    // "s3:GetObject" → "s3"). The lookup map is keyed by service
    // name, mirroring how the gateway authz path resolves them.
    let service = action.split(':').next().unwrap_or(action);
    let Some(lookup) = authz.resource_policy_lookups.get(service) else {
        return (None, None);
    };
    let Some(doc) = lookup.lookup(resource_arn) else {
        return (None, None);
    };
    let attr = PolicyAttribution {
        source_id: resource_arn.to_string(),
        source_type: PolicySource::Resource,
    };
    (Some(doc), Some(attr))
}

pub fn simulate_custom_policy(
    _state: &IamState,
    authz: Option<&Arc<AuthzEngine>>,
    input: &Value,
) -> Result<Value, AwsError> {
    let actions = extract_string_list(input, "ActionNames");
    let mut resources = extract_string_list(input, "ResourceArns");
    if resources.is_empty() {
        resources.push("*".to_string());
    }
    let context = extract_context_entries(input);

    // Treat each PolicyInputList entry as an inline identity-style
    // policy. Attribute by 1-based index since the user provides the
    // raw documents — we don't have a stable name to use.
    let identity_docs = parse_policy_input_list(input, "PolicyInputList")?;
    let identity_attrs: Vec<PolicyAttribution> = (0..identity_docs.len())
        .map(|i| PolicyAttribution {
            source_id: format!("PolicyInputList #{}", i + 1),
            source_type: PolicySource::Identity,
        })
        .collect();

    let bundle = AttributedPolicies {
        identity: identity_docs,
        identity_attributions: identity_attrs,
        permissions_boundary: None,
        permissions_boundary_attribution: None,
        scps: Vec::new(),
        scp_attributions: Vec::new(),
    };

    let results = build_evaluation_results(
        &bundle,
        &actions,
        &resources,
        "arn:aws:iam::000000000000:user/simulated",
        "000000000000",
        &context,
        authz,
    );

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

pub fn simulate_principal_policy(
    state: &IamState,
    authz: Option<&Arc<AuthzEngine>>,
    input: &Value,
) -> Result<Value, AwsError> {
    let principal_arn = require_str(input, "PolicySourceArn")?.to_string();
    let actions = extract_string_list(input, "ActionNames");
    let mut resources = extract_string_list(input, "ResourceArns");
    if resources.is_empty() {
        resources.push("*".to_string());
    }
    let context = extract_context_entries(input);

    // Inline PolicyInputList entries supplement the principal's own
    // policies — same as AWS, where you can add ad-hoc policies on
    // top of what the principal already has.
    let inline_docs = parse_policy_input_list(input, "PolicyInputList")?;
    let inline_attrs: Vec<PolicyAttribution> = (0..inline_docs.len())
        .map(|i| PolicyAttribution {
            source_id: format!("PolicyInputList #{}", i + 1),
            source_type: PolicySource::Identity,
        })
        .collect();

    let mut bundle = collect_principal_policies(state, &principal_arn)?;
    let principal_account = bundle_account(&principal_arn);

    bundle.identity.extend(inline_docs);
    bundle.identity_attributions.extend(inline_attrs);

    // SCPs — the gateway's Organizations lookup gives us the
    // org-scoped SCPs that apply to this principal. Same source we
    // use on the live request path, so the simulator output matches.
    if let Some(authz) = authz
        && let Some(scp_lookup) = authz.scp_lookup.as_ref()
    {
        for (i, doc) in scp_lookup.lookup(&principal_arn).into_iter().enumerate() {
            bundle.scps.push(doc);
            bundle.scp_attributions.push(PolicyAttribution {
                source_id: format!("SCP #{}", i + 1),
                source_type: PolicySource::Scp,
            });
        }
    }

    let results = build_evaluation_results(
        &bundle,
        &actions,
        &resources,
        &principal_arn,
        &principal_account,
        &context,
        authz,
    );

    Ok(json!({
        "EvaluationResults": { "member": results },
        "IsTruncated": false
    }))
}

fn bundle_account(arn: &str) -> String {
    arn.split(':')
        .nth(4)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "000000000000".to_string())
}

fn collect_principal_policies(state: &IamState, arn: &str) -> Result<AttributedPolicies, AwsError> {
    let mut bundle = AttributedPolicies {
        identity: Vec::new(),
        identity_attributions: Vec::new(),
        permissions_boundary: None,
        permissions_boundary_attribution: None,
        scps: Vec::new(),
        scp_attributions: Vec::new(),
    };

    let push_inline = |bundle: &mut AttributedPolicies,
                       prefix: &str,
                       name: &str,
                       policy_name: &str,
                       raw: &str|
     -> Result<(), AwsError> {
        let doc = parse_required(raw)?;
        bundle.identity.push(doc);
        bundle.identity_attributions.push(PolicyAttribution {
            source_id: format!("{prefix}/{name}/{policy_name}"),
            source_type: PolicySource::Identity,
        });
        Ok(())
    };
    let push_managed = |bundle: &mut AttributedPolicies,
                        policy_arn: &str,
                        doc_raw: &str|
     -> Result<(), AwsError> {
        let doc = parse_required(doc_raw)?;
        bundle.identity.push(doc);
        bundle.identity_attributions.push(PolicyAttribution {
            source_id: policy_arn.to_string(),
            source_type: PolicySource::Identity,
        });
        Ok(())
    };

    if let Some(user_entry) = state.users.iter().find(|e| e.value().arn == arn) {
        let user = user_entry.value().clone();
        for (policy_name, raw) in &user.inline_policies {
            push_inline(&mut bundle, "user", &user.user_name, policy_name, raw)?;
        }
        for policy_arn in &user.attached_policies {
            if let Some(p) = state.policies.get(policy_arn) {
                push_managed(&mut bundle, policy_arn, &p.value().policy_document)?;
            }
        }
        for group_name in &user.groups {
            if let Some(group) = state.groups.get(group_name) {
                let group = group.value();
                for (policy_name, raw) in &group.inline_policies {
                    push_inline(&mut bundle, "group", &group.group_name, policy_name, raw)?;
                }
                for policy_arn in &group.attached_policies {
                    if let Some(p) = state.policies.get(policy_arn) {
                        push_managed(&mut bundle, policy_arn, &p.value().policy_document)?;
                    }
                }
            }
        }
        if let Some(boundary_arn) = state.user_permissions_boundaries.get(&user.user_name)
            && let Some(p) = state.policies.get(boundary_arn.value())
        {
            bundle.permissions_boundary = Some(parse_required(&p.value().policy_document)?);
            bundle.permissions_boundary_attribution = Some(PolicyAttribution {
                source_id: boundary_arn.value().clone(),
                source_type: PolicySource::PermissionsBoundary,
            });
        }
        return Ok(bundle);
    }

    if let Some(role_entry) = state.roles.iter().find(|e| e.value().arn == arn) {
        let role = role_entry.value().clone();
        for (policy_name, raw) in &role.inline_policies {
            push_inline(&mut bundle, "role", &role.role_name, policy_name, raw)?;
        }
        for policy_arn in &role.attached_policies {
            if let Some(p) = state.policies.get(policy_arn) {
                push_managed(&mut bundle, policy_arn, &p.value().policy_document)?;
            }
        }
        if let Some(boundary_arn) = state.role_permissions_boundaries.get(&role.role_name)
            && let Some(p) = state.policies.get(boundary_arn.value())
        {
            bundle.permissions_boundary = Some(parse_required(&p.value().policy_document)?);
            bundle.permissions_boundary_attribution = Some(PolicyAttribution {
                source_id: boundary_arn.value().clone(),
                source_type: PolicySource::PermissionsBoundary,
            });
        }
        return Ok(bundle);
    }

    Err(no_such_entity("Principal", arn))
}

fn parse_required(raw: &str) -> Result<PolicyDocument, AwsError> {
    awsim_iam_policy::parse(raw)
        .map_err(|e| malformed_policy_document(format!("Syntax errors in policy. {e}")))
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
