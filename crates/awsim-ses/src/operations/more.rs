use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{
    ConfigurationSet, Contact, ContactList, CustomVerificationTemplate, DedicatedIpPool,
    EventDestination, SentEmail, SesState, SuppressedDestination,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn send_bulk_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let from = input["FromEmailAddress"]
        .as_str()
        .unwrap_or("noreply@awsim.local")
        .to_string();
    let entries = input["BulkEmailEntries"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for entry in entries {
        let to: Vec<String> = entry["Destination"]["ToAddresses"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let message_id = Uuid::new_v4().to_string();
        let email = SentEmail {
            message_id: message_id.clone(),
            from: from.clone(),
            to,
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            subject: None,
            body_text: None,
            body_html: None,
            raw: None,
            sent_at: now(),
            configuration_set_name: None,
            tags: vec![],
        };
        if let Some(store) = state.sqlite() {
            store.put_email(&ctx.account_id, &ctx.region, &email)?;
        }
        results.push(json!({
            "MessageId": message_id,
            "Status": "SUCCESS",
        }));
    }

    Ok(json!({ "BulkEmailEntryResults": results }))
}

pub fn send_custom_verification_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let template = input["TemplateName"].as_str().unwrap_or("default");
    let message_id = Uuid::new_v4().to_string();
    let entry = SentEmail {
        message_id: message_id.clone(),
        from: "verification@awsim.local".to_string(),
        to: vec![email.to_string()],
        cc: vec![],
        bcc: vec![],
        reply_to: vec![],
        subject: Some(format!("Verify {email} via {template}")),
        body_text: None,
        body_html: None,
        raw: None,
        sent_at: now(),
        configuration_set_name: None,
        tags: vec![],
    };
    if let Some(store) = state.sqlite() {
        store.put_email(&ctx.account_id, &ctx.region, &entry)?;
    }
    Ok(json!({ "MessageId": message_id }))
}

pub fn put_email_identity_dkim_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Email identity not found: {identity}"),
        )
    })?;
    if let Some(enabled) = input["SigningEnabled"].as_bool() {
        entry.dkim_signing_enabled = enabled;
    }
    Ok(json!({}))
}

/// PutEmailIdentityDkimSigningAttributes — switch the identity between
/// `AWS_SES` (EASY_DKIM) and `EXTERNAL` (BYODKIM). For EXTERNAL the
/// caller supplies a domain signing selector + private key, both of
/// which are persisted so subsequent GetEmailIdentity calls reflect the
/// state. AWS_SES bumps `DkimAttributes.NextSigningKeyLength` and
/// schedules a key rotation; we surface the requested key length without
/// modelling rotation.
pub fn put_email_identity_dkim_signing_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let origin = input["SigningAttributesOrigin"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameter",
            "SigningAttributesOrigin is required (AWS_SES or EXTERNAL)",
        )
    })?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Email identity not found: {identity}"),
        )
    })?;
    let attrs = &input["SigningAttributes"];
    match origin {
        "AWS_SES" => {
            let next_key = attrs["NextSigningKeyLength"]
                .as_str()
                .unwrap_or("RSA_2048_BIT");
            if !matches!(next_key, "RSA_1024_BIT" | "RSA_2048_BIT") {
                return Err(AwsError::bad_request(
                    "InvalidParameter",
                    format!(
                        "NextSigningKeyLength '{next_key}' must be RSA_1024_BIT or RSA_2048_BIT"
                    ),
                ));
            }
            entry.dkim_signing_attributes_origin = Some(origin.to_string());
            entry.dkim_next_signing_key_length = Some(next_key.to_string());
            entry.dkim_domain_signing_selector = None;
            entry.dkim_domain_signing_private_key = None;
            entry.dkim_status = Some("PENDING".to_string());
        }
        "EXTERNAL" => {
            let selector = attrs["DomainSigningSelector"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "SigningAttributes.DomainSigningSelector is required for EXTERNAL",
                )
            })?;
            let private_key = attrs["DomainSigningPrivateKey"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "SigningAttributes.DomainSigningPrivateKey is required for EXTERNAL",
                )
            })?;
            entry.dkim_signing_attributes_origin = Some(origin.to_string());
            entry.dkim_domain_signing_selector = Some(selector.to_string());
            entry.dkim_domain_signing_private_key = Some(private_key.to_string());
            entry.dkim_next_signing_key_length = None;
            entry.dkim_status = Some("SUCCESS".to_string());
        }
        other => {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                format!("SigningAttributesOrigin '{other}' must be AWS_SES or EXTERNAL"),
            ));
        }
    }
    Ok(json!({ "DkimStatus": entry.dkim_status, "DkimTokens": [] }))
}

/// Generate the three CNAME-style DKIM tokens for a domain identity and
/// move the verification status into the `Pending` slot. AWS responds
/// with the freshly issued tokens; calling this again refreshes them.
pub fn verify_domain_dkim(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let domain = input["Domain"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Domain is required"))?;
    let mut entry = state.identities.get_mut(domain).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Identity not found: {domain}"))
    })?;
    let tokens = generate_dkim_tokens();
    entry.dkim_tokens = tokens.clone();
    entry.dkim_status = Some("Pending".to_string());
    entry.dkim_signing_enabled = true;
    Ok(json!({ "DkimTokens": tokens }))
}

/// Return the DKIM attributes for one or more identities. AWS shape:
/// `DkimAttributes: { "<identity>": { DkimEnabled, DkimVerificationStatus, DkimTokens } }`.
pub fn get_identity_dkim_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identities = input["Identities"].as_array().cloned().unwrap_or_default();
    let mut attrs = serde_json::Map::new();
    for identity_value in identities {
        let Some(name) = identity_value.as_str() else {
            continue;
        };
        if let Some(entry) = state.identities.get(name) {
            attrs.insert(
                name.to_string(),
                json!({
                    "DkimEnabled": entry.dkim_signing_enabled,
                    "DkimVerificationStatus": entry
                        .dkim_status
                        .as_deref()
                        .unwrap_or("NotStarted"),
                    "DkimTokens": entry.dkim_tokens,
                }),
            );
        }
    }
    Ok(json!({ "DkimAttributes": Value::Object(attrs) }))
}

/// Toggle whether SES signs outbound mail for the identity with DKIM.
pub fn set_identity_dkim_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["Identity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Identity is required"))?;
    let enabled = input["DkimEnabled"]
        .as_bool()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "DkimEnabled is required"))?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    entry.dkim_signing_enabled = enabled;
    Ok(json!({}))
}

/// Awsim-specific helper that drives the DKIM verification state
/// machine deterministically. Useful for tests and operator scripts
/// that need to flip `Pending` to `Success` (DNS records published)
/// or `Failed` (DNS check timed out) without waiting on a real tick.
pub fn set_identity_dkim_verification(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["Identity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Identity is required"))?;
    let status = input["DkimVerificationStatus"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "DkimVerificationStatus is required")
    })?;
    if !matches!(
        status,
        "NotStarted" | "Pending" | "Success" | "Failed" | "TemporaryFailure"
    ) {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            format!(
                "DkimVerificationStatus '{status}' must be one of NotStarted, Pending, Success, Failed, TemporaryFailure"
            ),
        ));
    }
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    // Pending → Success requires tokens to have been issued; otherwise
    // AWS leaves the status untouched (we mirror that contract).
    if status == "Success" && entry.dkim_tokens.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Cannot mark identity Success before VerifyDomainDkim issued tokens",
        ));
    }
    entry.dkim_status = Some(status.to_string());
    Ok(json!({}))
}

fn generate_dkim_tokens() -> Vec<String> {
    (0..3)
        .map(|_| {
            uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(32)
                .collect::<String>()
        })
        .collect()
}

pub fn put_email_identity_mail_from_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Email identity not found: {identity}"),
        )
    })?;
    // BehaviorOnMxFailure: REJECT_MESSAGE | USE_DEFAULT_VALUE.
    let behavior = input["BehaviorOnMxFailure"].as_str();
    if let Some(b) = behavior
        && !matches!(b, "REJECT_MESSAGE" | "USE_DEFAULT_VALUE")
    {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("BehaviorOnMxFailure `{b}` must be REJECT_MESSAGE or USE_DEFAULT_VALUE."),
        ));
    }
    // MailFromDomain must end with the identity domain to be valid in
    // real SES (it has to live under the identity). Empty / null
    // values mean "remove MAIL FROM", which is allowed.
    let mail_from = input["MailFromDomain"].as_str();
    if let Some(mail_from) = mail_from
        && !mail_from.is_empty()
    {
        let identity_domain = identity.split_once('@').map(|(_, d)| d).unwrap_or(identity);
        if !mail_from
            .to_ascii_lowercase()
            .ends_with(&identity_domain.to_ascii_lowercase())
        {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!("MailFromDomain `{mail_from}` must be a subdomain of `{identity_domain}`."),
            ));
        }
    }
    entry.mail_from_domain = mail_from.filter(|s| !s.is_empty()).map(str::to_string);
    entry.mail_from_behavior_on_mx_failure = behavior.map(str::to_string);
    Ok(json!({}))
}

/// PutEmailIdentityConfigurationSetAttributes — attach a default
/// configuration set to an identity. Subsequent SendEmail calls that
/// omit `ConfigurationSetName` inherit this value.
pub fn put_email_identity_configuration_set_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Email identity not found: {identity}"),
        )
    })?;
    let cs_name = input["ConfigurationSetName"].as_str().map(str::to_string);
    if let Some(ref name) = cs_name
        && !state.configuration_sets.contains_key(name)
    {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Configuration set does not exist: {name}"),
        ));
    }
    entry.configuration_set_name = cs_name;
    Ok(json!({}))
}

pub fn put_email_identity_feedback_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn create_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let policy_name = input["PolicyName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PolicyName is required"))?;
    let policy = input["Policy"].as_str().unwrap_or("{}");
    state
        .identity_policies
        .entry(identity.to_string())
        .or_default()
        .insert(policy_name.to_string(), policy.to_string());
    Ok(json!({}))
}

pub fn delete_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"].as_str().unwrap_or("");
    let policy_name = input["PolicyName"].as_str().unwrap_or("");
    if let Some(mut entry) = state.identity_policies.get_mut(identity) {
        entry.remove(policy_name);
    }
    Ok(json!({}))
}

pub fn get_email_identity_policies(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"].as_str().unwrap_or("");
    let policies: HashMap<String, String> = state
        .identity_policies
        .get(identity)
        .map(|p| p.clone())
        .unwrap_or_default();
    let mut obj = serde_json::Map::new();
    for (k, v) in policies {
        obj.insert(k, Value::String(v));
    }
    Ok(json!({ "Policies": Value::Object(obj) }))
}

pub fn update_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    create_email_identity_policy(state, input, _ctx)
}

pub fn create_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
    })?;
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;
    let sending_enabled = input["SendingOptions"]["SendingEnabled"]
        .as_bool()
        .unwrap_or(true);
    let reputation_enabled = input["ReputationOptions"]["ReputationMetricsEnabled"]
        .as_bool()
        .unwrap_or(true);
    let tls_policy = parse_tls_policy(&input["DeliveryOptions"]["TlsPolicy"])?;
    let sending_pool_name = input["DeliveryOptions"]["SendingPoolName"]
        .as_str()
        .map(str::to_string);
    let vdm_dashboard_engagement_metrics =
        parse_enabled_disabled(&input["VdmOptions"]["DashboardOptions"]["EngagementMetrics"])?;
    let vdm_guardian_optimized_shared_delivery =
        parse_enabled_disabled(&input["VdmOptions"]["GuardianOptions"]["OptimizedSharedDelivery"])?;
    let mut cs = ConfigurationSet {
        name: name.to_string(),
        sending_enabled,
        reputation_metrics_enabled: reputation_enabled,
        reputation_last_fresh_start: if reputation_enabled {
            Some(now())
        } else {
            None
        },
        tls_policy,
        sending_pool_name,
        vdm_dashboard_engagement_metrics,
        vdm_guardian_optimized_shared_delivery,
        ..Default::default()
    };
    if let Some(t) = input["Tags"].as_array() {
        for tag in t {
            let k = tag["Key"].as_str().unwrap_or("").to_string();
            let v = tag["Value"].as_str().unwrap_or("").to_string();
            cs.tags.insert(k, v);
        }
    }
    state.configuration_sets.insert(name.to_string(), cs);
    Ok(json!({}))
}

/// Validate an `ENABLED` / `DISABLED` field, returning `None` when the
/// caller omitted it.
fn parse_enabled_disabled(value: &Value) -> Result<Option<String>, AwsError> {
    let Some(raw) = value.as_str() else {
        return Ok(None);
    };
    match raw {
        "ENABLED" | "DISABLED" => Ok(Some(raw.to_string())),
        other => Err(AwsError::bad_request(
            "InvalidParameter",
            format!("Value '{other}' must be ENABLED or DISABLED"),
        )),
    }
}

/// Validate `DeliveryOptions.TlsPolicy` against the AWS enum. Returns
/// `None` when the field is absent (configuration sets created without
/// DeliveryOptions inherit AWS's `OPTIONAL` default at send time).
fn parse_tls_policy(value: &Value) -> Result<Option<String>, AwsError> {
    let Some(raw) = value.as_str() else {
        return Ok(None);
    };
    match raw {
        "REQUIRE" | "OPTIONAL" => Ok(Some(raw.to_string())),
        other => Err(AwsError::bad_request(
            "InvalidParameter",
            format!("TlsPolicy '{other}' must be REQUIRE or OPTIONAL"),
        )),
    }
}

/// PutConfigurationSetDeliveryOptions — update TLS policy + sending pool
/// on an existing configuration set. AWS keeps unspecified fields as-is
/// when omitted, but in awsim we treat the request as a full replace per
/// the v2 API contract.
pub fn put_configuration_set_delivery_options(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
    })?;
    let tls_policy = parse_tls_policy(&input["TlsPolicy"])?;
    let sending_pool_name = input["SendingPoolName"].as_str().map(str::to_string);
    let mut cs = state
        .configuration_sets
        .get_mut(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    cs.tls_policy = tls_policy;
    cs.sending_pool_name = sending_pool_name;
    Ok(json!({}))
}

/// PutConfigurationSetVdmOptions — replace VDM options for a set.
pub fn put_configuration_set_vdm_options(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
    })?;
    let dashboard =
        parse_enabled_disabled(&input["VdmOptions"]["DashboardOptions"]["EngagementMetrics"])?;
    let guardian =
        parse_enabled_disabled(&input["VdmOptions"]["GuardianOptions"]["OptimizedSharedDelivery"])?;
    let mut cs = state
        .configuration_sets
        .get_mut(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    cs.vdm_dashboard_engagement_metrics = dashboard;
    cs.vdm_guardian_optimized_shared_delivery = guardian;
    Ok(json!({}))
}

pub fn put_configuration_set_reputation_options(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
    })?;
    let enabled = input["ReputationMetricsEnabled"].as_bool().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ReputationMetricsEnabled is required")
    })?;
    let mut cs = state
        .configuration_sets
        .get_mut(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    let was_enabled = cs.reputation_metrics_enabled;
    cs.reputation_metrics_enabled = enabled;
    if enabled && !was_enabled {
        cs.reputation_last_fresh_start = Some(now());
    }
    Ok(json!({}))
}

pub fn delete_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().unwrap_or("");
    state.configuration_sets.remove(name);
    Ok(json!({}))
}

pub fn get_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
    })?;
    let cs = state
        .configuration_sets
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    let tags: Vec<Value> = cs
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();
    let mut reputation = json!({
        "ReputationMetricsEnabled": cs.reputation_metrics_enabled,
    });
    if let Some(ts) = cs.reputation_last_fresh_start {
        reputation["LastFreshStart"] = json!(ts);
    }
    let mut delivery = serde_json::Map::new();
    if let Some(ref policy) = cs.tls_policy {
        delivery.insert("TlsPolicy".to_string(), json!(policy));
    }
    if let Some(ref pool) = cs.sending_pool_name {
        delivery.insert("SendingPoolName".to_string(), json!(pool));
    }
    let mut vdm = serde_json::Map::new();
    if let Some(ref m) = cs.vdm_dashboard_engagement_metrics {
        vdm.insert(
            "DashboardOptions".to_string(),
            json!({ "EngagementMetrics": m }),
        );
    }
    if let Some(ref g) = cs.vdm_guardian_optimized_shared_delivery {
        vdm.insert(
            "GuardianOptions".to_string(),
            json!({ "OptimizedSharedDelivery": g }),
        );
    }
    Ok(json!({
        "ConfigurationSetName": cs.name,
        "Tags": tags,
        "SendingOptions": { "SendingEnabled": cs.sending_enabled },
        "ReputationOptions": reputation,
        "DeliveryOptions": Value::Object(delivery),
        "VdmOptions": Value::Object(vdm),
    }))
}

pub fn list_configuration_sets(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = state
        .configuration_sets
        .iter()
        .map(|e| e.key().clone())
        .collect();
    Ok(json!({ "ConfigurationSets": names }))
}

/// AWS SES configuration-set event-destination `MatchingEventTypes`
/// enum. Unknown values are rejected at create time so a misspelled
/// type fails loudly instead of silently swallowing every event.
const VALID_EVENT_TYPES: &[&str] = &[
    "SEND",
    "REJECT",
    "BOUNCE",
    "COMPLAINT",
    "DELIVERY",
    "OPEN",
    "CLICK",
    "RENDERING_FAILURE",
    "DELIVERY_DELAY",
    "SUBSCRIPTION",
];

pub fn create_configuration_set_event_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let dest_name = input["EventDestinationName"].as_str().unwrap_or("default");
    let event_dest = &input["EventDestination"];
    let event_types: Vec<String> = event_dest["MatchingEventTypes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for t in &event_types {
        if !VALID_EVENT_TYPES.contains(&t.as_str()) {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!("Invalid event type: {t}"),
            ));
        }
    }

    // Parse the target sub-object. AWS allows exactly one of these per
    // event destination; we store whichever is present so the send path
    // can fan out to it.
    let sns_topic_arn = event_dest["SnsDestination"]["TopicArn"]
        .as_str()
        .map(String::from);
    let firehose_delivery_stream_arn =
        event_dest["KinesisFirehoseDestination"]["DeliveryStreamArn"]
            .as_str()
            .map(String::from);
    let cloudwatch_dimensions = event_dest["CloudWatchDestination"]["DimensionConfigurations"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut cs = state.configuration_sets.get_mut(cs_name).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Configuration set does not exist: {cs_name}"),
        )
    })?;
    cs.event_destinations.push(EventDestination {
        name: dest_name.to_string(),
        enabled: event_dest["Enabled"].as_bool().unwrap_or(true),
        matching_event_types: event_types,
        sns_topic_arn,
        firehose_delivery_stream_arn,
        cloudwatch_dimensions,
    });
    Ok(json!({}))
}

pub fn delete_configuration_set_event_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let dest_name = input["EventDestinationName"].as_str().unwrap_or("");
    if let Some(mut cs) = state.configuration_sets.get_mut(cs_name) {
        cs.event_destinations.retain(|d| d.name != dest_name);
    }
    Ok(json!({}))
}

pub fn get_configuration_set_event_destinations(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let destinations: Vec<Value> = state
        .configuration_sets
        .get(cs_name)
        .map(|cs| {
            cs.event_destinations
                .iter()
                .map(|d| {
                    let mut obj = json!({
                        "Name": d.name,
                        "Enabled": d.enabled,
                        "MatchingEventTypes": d.matching_event_types,
                    });
                    if let Some(arn) = &d.sns_topic_arn {
                        obj["SnsDestination"] = json!({ "TopicArn": arn });
                    }
                    if let Some(arn) = &d.firehose_delivery_stream_arn {
                        obj["KinesisFirehoseDestination"] = json!({ "DeliveryStreamArn": arn });
                    }
                    if !d.cloudwatch_dimensions.is_empty() {
                        obj["CloudWatchDestination"] =
                            json!({ "DimensionConfigurations": d.cloudwatch_dimensions });
                    }
                    obj
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "EventDestinations": destinations }))
}

pub fn create_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PoolName is required"))?;
    let scaling_mode = input["ScalingMode"]
        .as_str()
        .unwrap_or("STANDARD")
        .to_string();
    let pool = DedicatedIpPool {
        name: name.to_string(),
        scaling_mode,
        ips: vec!["192.0.2.1".to_string(), "192.0.2.2".to_string()],
    };
    state.dedicated_ip_pools.insert(name.to_string(), pool);
    Ok(json!({}))
}

pub fn delete_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    state.dedicated_ip_pools.remove(name);
    Ok(json!({}))
}

pub fn get_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    let pool = state
        .dedicated_ip_pools
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "DedicatedIpPool": {
            "PoolName": pool.name,
            "ScalingMode": pool.scaling_mode,
        }
    }))
}

pub fn list_dedicated_ip_pools(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = state
        .dedicated_ip_pools
        .iter()
        .map(|e| e.key().clone())
        .collect();
    Ok(json!({ "DedicatedIpPools": names }))
}

pub fn get_dedicated_ips(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    let ips: Vec<Value> = state
        .dedicated_ip_pools
        .get(name)
        .map(|p| {
            p.ips
                .iter()
                .map(|ip| {
                    json!({
                        "Ip": ip,
                        "WarmupStatus": "DONE",
                        "WarmupPercentage": 100,
                        "PoolName": p.name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "DedicatedIps": ips }))
}

pub fn put_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let reason = input["Reason"].as_str().unwrap_or("BOUNCE").to_string();
    if !matches!(reason.as_str(), "BOUNCE" | "COMPLAINT") {
        return Err(AwsError::bad_request(
            "BadRequestException",
            format!("Reason `{reason}` must be BOUNCE or COMPLAINT."),
        ));
    }
    state.suppressed_destinations.insert(
        email.to_string(),
        SuppressedDestination {
            email: email.to_string(),
            reason,
            last_update: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"].as_str().unwrap_or("");
    state.suppressed_destinations.remove(email);
    Ok(json!({}))
}

pub fn get_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"].as_str().unwrap_or("");
    let s = state
        .suppressed_destinations
        .get(email)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {email}")))?;
    Ok(json!({
        "SuppressedDestination": {
            "EmailAddress": s.email,
            "Reason": s.reason,
            "LastUpdateTime": s.last_update,
        }
    }))
}

pub fn list_suppressed_destinations(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .suppressed_destinations
        .iter()
        .map(|e| {
            json!({
                "EmailAddress": e.value().email,
                "Reason": e.value().reason,
                "LastUpdateTime": e.value().last_update,
            })
        })
        .collect();
    Ok(json!({ "SuppressedDestinationSummaries": summaries }))
}

pub fn create_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ContactListName is required"))?;
    let description = input["Description"].as_str().map(String::from);
    let topics = input["Topics"].as_array().cloned().unwrap_or_default();
    state.contact_lists.insert(
        name.to_string(),
        ContactList {
            name: name.to_string(),
            description,
            topics,
            created_at: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    state.contact_lists.remove(name);
    Ok(json!({}))
}

pub fn get_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    let cl = state
        .contact_lists
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "ContactListName": cl.name,
        "Description": cl.description,
        "Topics": cl.topics,
        "CreatedTimestamp": cl.created_at,
    }))
}

pub fn list_contact_lists(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let lists: Vec<Value> = state
        .contact_lists
        .iter()
        .map(|e| {
            json!({
                "ContactListName": e.value().name,
                "LastUpdatedTimestamp": e.value().created_at,
            })
        })
        .collect();
    Ok(json!({ "ContactLists": lists }))
}

pub fn update_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    if let Some(mut cl) = state.contact_lists.get_mut(name) {
        if let Some(d) = input["Description"].as_str() {
            cl.description = Some(d.to_string());
        }
        if let Some(t) = input["Topics"].as_array() {
            cl.topics = t.clone();
        }
    }
    Ok(json!({}))
}

fn contact_key(list: &str, email: &str) -> String {
    format!("{list}#{email}")
}

pub fn create_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let topic_prefs = input["TopicPreferences"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let unsubscribe = input["UnsubscribeAll"].as_bool().unwrap_or(false);
    let attributes = input["AttributesData"].as_str().map(String::from);
    state.contacts.insert(
        contact_key(list_name, email),
        Contact {
            email: email.to_string(),
            list_name: list_name.to_string(),
            topic_preferences: topic_prefs,
            unsubscribe_all: unsubscribe,
            attributes,
            created_at: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    state.contacts.remove(&contact_key(list_name, email));
    Ok(json!({}))
}

pub fn get_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    let c = state
        .contacts
        .get(&contact_key(list_name, email))
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {email}")))?;
    Ok(json!({
        "ContactListName": c.list_name,
        "EmailAddress": c.email,
        "TopicPreferences": c.topic_preferences,
        "UnsubscribeAll": c.unsubscribe_all,
        "AttributesData": c.attributes,
        "CreatedTimestamp": c.created_at,
    }))
}

pub fn list_contacts(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let contacts: Vec<Value> = state
        .contacts
        .iter()
        .filter(|e| e.value().list_name == list_name)
        .map(|e| {
            json!({
                "EmailAddress": e.value().email,
                "TopicPreferences": e.value().topic_preferences,
                "UnsubscribeAll": e.value().unsubscribe_all,
            })
        })
        .collect();
    Ok(json!({ "Contacts": contacts }))
}

pub fn update_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    if let Some(mut c) = state.contacts.get_mut(&contact_key(list_name, email)) {
        if let Some(t) = input["TopicPreferences"].as_array() {
            c.topic_preferences = t.clone();
        }
        if let Some(u) = input["UnsubscribeAll"].as_bool() {
            c.unsubscribe_all = u;
        }
        if let Some(a) = input["AttributesData"].as_str() {
            c.attributes = Some(a.to_string());
        }
    }
    Ok(json!({}))
}

pub fn create_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;
    let cv = CustomVerificationTemplate {
        name: name.to_string(),
        from: input["FromEmailAddress"].as_str().unwrap_or("").to_string(),
        subject: input["TemplateSubject"].as_str().unwrap_or("").to_string(),
        content: input["TemplateContent"].as_str().unwrap_or("").to_string(),
        success_url: input["SuccessRedirectionURL"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        failure_url: input["FailureRedirectionURL"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };
    state
        .custom_verification_templates
        .insert(name.to_string(), cv);
    Ok(json!({}))
}

pub fn delete_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"].as_str().unwrap_or("");
    state.custom_verification_templates.remove(name);
    Ok(json!({}))
}

pub fn get_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"].as_str().unwrap_or("");
    let cv = state
        .custom_verification_templates
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "TemplateName": cv.name,
        "FromEmailAddress": cv.from,
        "TemplateSubject": cv.subject,
        "TemplateContent": cv.content,
        "SuccessRedirectionURL": cv.success_url,
        "FailureRedirectionURL": cv.failure_url,
    }))
}

pub fn list_custom_verification_email_templates(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let templates: Vec<Value> = state
        .custom_verification_templates
        .iter()
        .map(|e| {
            json!({
                "TemplateName": e.value().name,
                "FromEmailAddress": e.value().from,
                "TemplateSubject": e.value().subject,
                "SuccessRedirectionURL": e.value().success_url,
                "FailureRedirectionURL": e.value().failure_url,
            })
        })
        .collect();
    Ok(json!({ "CustomVerificationEmailTemplates": templates }))
}

pub fn update_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;
    let content = &input["TemplateContent"];
    if let Some(mut t) = state.templates.get_mut(name) {
        if let Some(s) = content["Subject"].as_str() {
            t.subject = Some(s.to_string());
        }
        if let Some(h) = content["Html"].as_str() {
            t.html = Some(h.to_string());
        }
        if let Some(tx) = content["Text"].as_str() {
            t.text = Some(tx.to_string());
        }
    }
    Ok(json!({}))
}

pub fn tag_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    validate_aws_tags(&input["Tags"], &TagOpts::aws_default())?;
    let tags = input["Tags"].as_array().cloned().unwrap_or_default();
    let mut entry = state.identity_tags.entry(arn.to_string()).or_default();
    for tag in tags {
        let k = tag["Key"].as_str().unwrap_or("").to_string();
        let v = tag["Value"].as_str().unwrap_or("").to_string();
        entry.insert(k, v);
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    validate_aws_tag_keys(&input["TagKeys"])?;
    let keys = input["TagKeys"].as_array().cloned().unwrap_or_default();
    if let Some(mut entry) = state.identity_tags.get_mut(arn) {
        for k in keys {
            if let Some(s) = k.as_str() {
                entry.remove(s);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    let tags: Vec<Value> = state
        .identity_tags
        .get(arn)
        .map(|t| {
            t.iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "Tags": tags }))
}

pub fn put_account_sending_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn put_account_suppression_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let reasons = input["SuppressedReasons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for r in &reasons {
        let s = r.as_str().unwrap_or("");
        if !matches!(s, "BOUNCE" | "COMPLAINT") {
            return Err(AwsError::bad_request(
                "BadRequestException",
                format!("SuppressedReasons entry `{s}` must be BOUNCE or COMPLAINT."),
            ));
        }
    }
    *state.account_suppression_attributes.lock().unwrap() =
        Some(json!({ "SuppressedReasons": reasons }));
    Ok(json!({}))
}

pub fn put_account_vdm_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let vdm = input["VdmAttributes"].clone();
    if !vdm.is_object() {
        return Err(AwsError::bad_request(
            "BadRequestException",
            "VdmAttributes is required and must be an object.",
        ));
    }
    let enabled = vdm["VdmEnabled"].as_str().unwrap_or("");
    if !matches!(enabled, "ENABLED" | "DISABLED") {
        return Err(AwsError::bad_request(
            "BadRequestException",
            "VdmAttributes.VdmEnabled must be ENABLED or DISABLED.",
        ));
    }
    *state.account_vdm_attributes.lock().unwrap() = Some(vdm);
    Ok(json!({}))
}

pub fn put_account_dedicated_ip_warmup_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn get_deliverability_dashboard_options(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "DashboardEnabled": false,
        "ActiveSubscribedDomains": [],
        "PendingExpirationSubscribedDomains": [],
    }))
}

pub fn put_deliverability_dashboard_option(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn get_blacklist_reports(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "BlacklistReport": {} }))
}

#[cfg(test)]
mod reputation_options_tests {
    use super::*;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    #[test]
    fn create_configuration_set_persists_reputation_options() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "ReputationOptions": { "ReputationMetricsEnabled": false },
            }),
            &ctx(),
        )
        .unwrap();
        let cs = state.configuration_sets.get("cs").unwrap();
        assert!(!cs.reputation_metrics_enabled);
        assert!(cs.reputation_last_fresh_start.is_none());
    }

    #[test]
    fn put_configuration_set_reputation_options_sets_fresh_start_on_toggle_on() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "ReputationOptions": { "ReputationMetricsEnabled": false },
            }),
            &ctx(),
        )
        .unwrap();
        put_configuration_set_reputation_options(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "ReputationMetricsEnabled": true,
            }),
            &ctx(),
        )
        .unwrap();
        let cs = state.configuration_sets.get("cs").unwrap();
        assert!(cs.reputation_metrics_enabled);
        assert!(cs.reputation_last_fresh_start.is_some());
    }

    #[test]
    fn put_configuration_set_reputation_options_returns_not_found_for_missing_set() {
        let state = SesState::default();
        let err = put_configuration_set_reputation_options(
            &state,
            &json!({
                "ConfigurationSetName": "nope",
                "ReputationMetricsEnabled": true,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }

    #[test]
    fn get_configuration_set_surfaces_last_fresh_start() {
        let state = SesState::default();
        create_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx()).unwrap();
        let resp = get_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx())
            .unwrap();
        assert!(
            resp["ReputationOptions"].get("LastFreshStart").is_some(),
            "LastFreshStart should appear when reputation is enabled"
        );
    }
}

#[cfg(test)]
mod delivery_options_tests {
    use super::*;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    #[test]
    fn create_configuration_set_persists_tls_policy() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "DeliveryOptions": { "TlsPolicy": "REQUIRE", "SendingPoolName": "pool-1" },
            }),
            &ctx(),
        )
        .unwrap();
        let cs = state.configuration_sets.get("cs").unwrap();
        assert_eq!(cs.tls_policy.as_deref(), Some("REQUIRE"));
        assert_eq!(cs.sending_pool_name.as_deref(), Some("pool-1"));
    }

    #[test]
    fn create_configuration_set_rejects_invalid_tls_policy() {
        let state = SesState::default();
        let err = create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "DeliveryOptions": { "TlsPolicy": "STRICT" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("TlsPolicy"));
    }

    #[test]
    fn put_delivery_options_updates_policy_and_pool() {
        let state = SesState::default();
        create_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx()).unwrap();
        put_configuration_set_delivery_options(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "TlsPolicy": "OPTIONAL",
                "SendingPoolName": "pool-2",
            }),
            &ctx(),
        )
        .unwrap();
        let cs = state.configuration_sets.get("cs").unwrap();
        assert_eq!(cs.tls_policy.as_deref(), Some("OPTIONAL"));
        assert_eq!(cs.sending_pool_name.as_deref(), Some("pool-2"));
    }

    #[test]
    fn put_delivery_options_not_found_for_missing_set() {
        let state = SesState::default();
        let err = put_configuration_set_delivery_options(
            &state,
            &json!({ "ConfigurationSetName": "nope", "TlsPolicy": "REQUIRE" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }

    #[test]
    fn get_configuration_set_surfaces_delivery_options() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "DeliveryOptions": { "TlsPolicy": "REQUIRE" },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = get_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx())
            .unwrap();
        assert_eq!(resp["DeliveryOptions"]["TlsPolicy"], "REQUIRE");
    }
}

#[cfg(test)]
mod dkim_verification_state_machine_tests {
    use super::*;
    use crate::operations::identities::create_email_identity;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    #[test]
    fn verify_domain_dkim_issues_tokens_and_pending_status() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        let resp = verify_domain_dkim(&state, &json!({ "Domain": "example.com" }), &ctx()).unwrap();
        let tokens = resp["DkimTokens"].as_array().unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(tokens.iter().all(|t| t.as_str().unwrap().len() == 32));
        let entry = state.identities.get("example.com").unwrap();
        assert_eq!(entry.dkim_status.as_deref(), Some("Pending"));
        assert_eq!(entry.dkim_tokens.len(), 3);
    }

    #[test]
    fn pending_transitions_to_success_only_after_tokens_issued() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        let err = set_identity_dkim_verification(
            &state,
            &json!({ "Identity": "example.com", "DkimVerificationStatus": "Success" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");

        verify_domain_dkim(&state, &json!({ "Domain": "example.com" }), &ctx()).unwrap();
        set_identity_dkim_verification(
            &state,
            &json!({ "Identity": "example.com", "DkimVerificationStatus": "Success" }),
            &ctx(),
        )
        .unwrap();
        let entry = state.identities.get("example.com").unwrap();
        assert_eq!(entry.dkim_status.as_deref(), Some("Success"));
    }

    #[test]
    fn pending_transitions_to_failed_or_temporary_failure() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        verify_domain_dkim(&state, &json!({ "Domain": "example.com" }), &ctx()).unwrap();
        for status in ["Failed", "TemporaryFailure", "Pending", "NotStarted"] {
            set_identity_dkim_verification(
                &state,
                &json!({ "Identity": "example.com", "DkimVerificationStatus": status }),
                &ctx(),
            )
            .unwrap();
            let entry = state.identities.get("example.com").unwrap();
            assert_eq!(entry.dkim_status.as_deref(), Some(status));
        }
    }

    #[test]
    fn rejects_unknown_verification_status() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        let err = set_identity_dkim_verification(
            &state,
            &json!({ "Identity": "example.com", "DkimVerificationStatus": "Maybe" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn get_identity_dkim_attributes_surfaces_state_and_tokens() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        verify_domain_dkim(&state, &json!({ "Domain": "example.com" }), &ctx()).unwrap();
        let resp = get_identity_dkim_attributes(
            &state,
            &json!({ "Identities": ["example.com", "missing.com"] }),
            &ctx(),
        )
        .unwrap();
        let attrs = resp["DkimAttributes"].as_object().unwrap();
        assert!(attrs.contains_key("example.com"));
        assert!(!attrs.contains_key("missing.com"));
        let row = &attrs["example.com"];
        assert_eq!(row["DkimEnabled"], true);
        assert_eq!(row["DkimVerificationStatus"], "Pending");
        assert_eq!(row["DkimTokens"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn set_identity_dkim_enabled_toggles_signing() {
        let state = SesState::default();
        create_email_identity(&state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
        set_identity_dkim_enabled(
            &state,
            &json!({ "Identity": "example.com", "DkimEnabled": false }),
            &ctx(),
        )
        .unwrap();
        let entry = state.identities.get("example.com").unwrap();
        assert!(!entry.dkim_signing_enabled);
    }
}

#[cfg(test)]
mod dkim_signing_attributes_tests {
    use super::*;
    use crate::operations::identities::create_email_identity;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    fn seed_identity(state: &SesState) {
        create_email_identity(state, &json!({ "EmailIdentity": "example.com" }), &ctx()).unwrap();
    }

    #[test]
    fn easy_dkim_records_next_key_length_and_pending_status() {
        let state = SesState::default();
        seed_identity(&state);
        put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "AWS_SES",
                "SigningAttributes": { "NextSigningKeyLength": "RSA_1024_BIT" },
            }),
            &ctx(),
        )
        .unwrap();
        let entry = state.identities.get("example.com").unwrap();
        assert_eq!(
            entry.dkim_signing_attributes_origin.as_deref(),
            Some("AWS_SES")
        );
        assert_eq!(
            entry.dkim_next_signing_key_length.as_deref(),
            Some("RSA_1024_BIT")
        );
        assert_eq!(entry.dkim_status.as_deref(), Some("PENDING"));
    }

    #[test]
    fn byodkim_stores_selector_and_private_key() {
        let state = SesState::default();
        seed_identity(&state);
        put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "EXTERNAL",
                "SigningAttributes": {
                    "DomainSigningSelector": "selector1",
                    "DomainSigningPrivateKey": "MIIEvgIBADANBgkqhkiG9w0BAQEFAASC..."
                },
            }),
            &ctx(),
        )
        .unwrap();
        let entry = state.identities.get("example.com").unwrap();
        assert_eq!(
            entry.dkim_signing_attributes_origin.as_deref(),
            Some("EXTERNAL")
        );
        assert_eq!(
            entry.dkim_domain_signing_selector.as_deref(),
            Some("selector1")
        );
        assert!(
            entry
                .dkim_domain_signing_private_key
                .as_deref()
                .unwrap_or("")
                .starts_with("MIIEvgIBADANBg")
        );
        assert_eq!(entry.dkim_status.as_deref(), Some("SUCCESS"));
    }

    #[test]
    fn byodkim_requires_selector_and_key() {
        let state = SesState::default();
        seed_identity(&state);
        let err = put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "EXTERNAL",
                "SigningAttributes": { "DomainSigningSelector": "sel" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("DomainSigningPrivateKey"));
    }

    #[test]
    fn rejects_unknown_origin() {
        let state = SesState::default();
        seed_identity(&state);
        let err = put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "WHATEVER",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn rejects_invalid_next_key_length() {
        let state = SesState::default();
        seed_identity(&state);
        let err = put_email_identity_dkim_signing_attributes(
            &state,
            &json!({
                "EmailIdentity": "example.com",
                "SigningAttributesOrigin": "AWS_SES",
                "SigningAttributes": { "NextSigningKeyLength": "RSA_4096_BIT" },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("NextSigningKeyLength"));
    }
}

#[cfg(test)]
mod vdm_options_tests {
    use super::*;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    #[test]
    fn create_configuration_set_persists_vdm_options() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "VdmOptions": {
                    "DashboardOptions": { "EngagementMetrics": "ENABLED" },
                    "GuardianOptions": { "OptimizedSharedDelivery": "DISABLED" },
                },
            }),
            &ctx(),
        )
        .unwrap();
        let cs = state.configuration_sets.get("cs").unwrap();
        assert_eq!(
            cs.vdm_dashboard_engagement_metrics.as_deref(),
            Some("ENABLED")
        );
        assert_eq!(
            cs.vdm_guardian_optimized_shared_delivery.as_deref(),
            Some("DISABLED")
        );
    }

    #[test]
    fn rejects_invalid_vdm_enum() {
        let state = SesState::default();
        let err = create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "VdmOptions": {
                    "DashboardOptions": { "EngagementMetrics": "ON" },
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn put_vdm_options_updates_existing_set() {
        let state = SesState::default();
        create_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx()).unwrap();
        put_configuration_set_vdm_options(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "VdmOptions": {
                    "DashboardOptions": { "EngagementMetrics": "ENABLED" },
                    "GuardianOptions": { "OptimizedSharedDelivery": "ENABLED" },
                },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = get_configuration_set(&state, &json!({ "ConfigurationSetName": "cs" }), &ctx())
            .unwrap();
        assert_eq!(
            resp["VdmOptions"]["DashboardOptions"]["EngagementMetrics"],
            "ENABLED"
        );
        assert_eq!(
            resp["VdmOptions"]["GuardianOptions"]["OptimizedSharedDelivery"],
            "ENABLED"
        );
    }

    #[test]
    fn put_vdm_options_not_found_for_missing_set() {
        let state = SesState::default();
        let err = put_configuration_set_vdm_options(
            &state,
            &json!({
                "ConfigurationSetName": "nope",
                "VdmOptions": { "DashboardOptions": { "EngagementMetrics": "ENABLED" } },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }
}

#[cfg(test)]
mod tag_validation_tests {
    use super::*;

    fn ctx() -> awsim_core::RequestContext {
        awsim_core::RequestContext::new("ses", "us-east-1")
    }

    #[test]
    fn tag_resource_rejects_reserved_aws_prefix() {
        let state = SesState::default();
        let err = tag_resource(
            &state,
            &json!({
                "ResourceArn": "arn:aws:ses:us-east-1:000000000000:identity/example.com",
                "Tags": [{ "Key": "aws:reserved", "Value": "v" }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_configuration_set_rejects_reserved_aws_prefix() {
        let state = SesState::default();
        let err = create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "Tags": [{ "Key": "aws:reserved", "Value": "v" }],
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(state.configuration_sets.get("cs").is_none());
    }
}

#[cfg(test)]
mod event_destination_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn make_cs(state: &SesState) {
        create_configuration_set(state, &json!({ "ConfigurationSetName": "cs" }), &ctx()).unwrap();
    }

    #[test]
    fn create_persists_sns_target_and_round_trips() {
        let state = SesState::default();
        make_cs(&state);
        create_configuration_set_event_destination(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "EventDestinationName": "d1",
                "EventDestination": {
                    "Enabled": true,
                    "MatchingEventTypes": ["SEND", "DELIVERY"],
                    "SnsDestination": { "TopicArn": "arn:aws:sns:us-east-1:000000000000:t" },
                },
            }),
            &ctx(),
        )
        .unwrap();
        let out = get_configuration_set_event_destinations(
            &state,
            &json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .unwrap();
        let d = &out["EventDestinations"][0];
        assert_eq!(d["Name"], "d1");
        assert_eq!(
            d["SnsDestination"]["TopicArn"],
            "arn:aws:sns:us-east-1:000000000000:t"
        );
        assert_eq!(d["MatchingEventTypes"][0], "SEND");
    }

    #[test]
    fn create_persists_firehose_and_cloudwatch_targets() {
        let state = SesState::default();
        make_cs(&state);
        create_configuration_set_event_destination(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "EventDestinationName": "fh",
                "EventDestination": {
                    "MatchingEventTypes": ["BOUNCE"],
                    "KinesisFirehoseDestination": {
                        "DeliveryStreamArn": "arn:aws:firehose:us-east-1:000000000000:deliverystream/s1",
                        "IamRoleArn": "arn:aws:iam::000000000000:role/r",
                    },
                },
            }),
            &ctx(),
        )
        .unwrap();
        create_configuration_set_event_destination(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "EventDestinationName": "cw",
                "EventDestination": {
                    "MatchingEventTypes": ["OPEN"],
                    "CloudWatchDestination": {
                        "DimensionConfigurations": [
                            { "DimensionName": "ses:configuration-set", "DimensionValueSource": "MESSAGE_TAG", "DefaultDimensionValue": "cs" }
                        ]
                    },
                },
            }),
            &ctx(),
        )
        .unwrap();
        let out = get_configuration_set_event_destinations(
            &state,
            &json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .unwrap();
        let dests = out["EventDestinations"].as_array().unwrap();
        assert_eq!(dests.len(), 2);
        let fh = dests.iter().find(|d| d["Name"] == "fh").unwrap();
        assert_eq!(
            fh["KinesisFirehoseDestination"]["DeliveryStreamArn"],
            "arn:aws:firehose:us-east-1:000000000000:deliverystream/s1"
        );
        let cw = dests.iter().find(|d| d["Name"] == "cw").unwrap();
        assert_eq!(
            cw["CloudWatchDestination"]["DimensionConfigurations"][0]["DimensionName"],
            "ses:configuration-set"
        );
    }

    #[test]
    fn create_rejects_unknown_event_type() {
        let state = SesState::default();
        make_cs(&state);
        let err = create_configuration_set_event_destination(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "EventDestinationName": "bad",
                "EventDestination": { "MatchingEventTypes": ["FOO"] },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn create_returns_not_found_for_missing_configuration_set() {
        let state = SesState::default();
        let err = create_configuration_set_event_destination(
            &state,
            &json!({
                "ConfigurationSetName": "nope",
                "EventDestinationName": "d1",
                "EventDestination": { "MatchingEventTypes": ["SEND"] },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }
}
