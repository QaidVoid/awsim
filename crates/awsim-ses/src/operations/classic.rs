//! SES classic (v1) operations.
//!
//! SES has two APIs over the same account state: the v2 REST/JSON API
//! and the original query API that `aws ses ...` still speaks. AWSim
//! covered v2 well but almost none of v1, so identity management, the
//! entry point for every SES workflow, returned
//! `UnknownOperationException` and nothing else could be reached.
//!
//! These handlers read and write the same [`SesState`] as their v2
//! counterparts. Only the wire shape differs: v1 is the query protocol,
//! where a list needs a `member` wrapper and a map is a list of
//! `entry` key/value pairs.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Map, Value, json};
use tracing::info;

use crate::state::{EmailIdentity, EmailTemplate, ReceiptFilter, SesState};

use super::emails::render_template;

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", format!("{key} is required")))
}

/// Render a string-keyed map the way the query protocol serializes one.
///
/// Entries are sorted by key so repeated calls return the same document.
fn entries(map: impl IntoIterator<Item = (String, Value)>) -> Value {
    let mut pairs: Vec<(String, Value)> = map.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    json!({
        "entry": pairs
            .into_iter()
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect::<Vec<_>>(),
    })
}

/// Read a query-protocol list, which arrives as a bare array once the
/// parser has unwrapped its `member` container.
fn string_list(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A verification token for a domain identity.
///
/// AWS issues an opaque 44-character base32 string. Callers only ever
/// echo it into a DNS record, so the one property that matters is that
/// the same domain always yields the same token: a test that provisions
/// twice must not see the value change.
fn verification_token(domain: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in domain.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    let mut out = String::with_capacity(44);
    for i in 0..44 {
        let idx = ((hash >> (i % 8 * 8)) as usize).wrapping_add(i) % ALPHABET.len();
        out.push(ALPHABET[idx] as char);
        if i % 8 == 7 {
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
    }
    out
}

fn is_domain(identity: &str) -> bool {
    !identity.contains('@')
}

/// Create the identity if it is new, leaving an existing one untouched.
///
/// AWS treats a repeated Verify call as a no-op rather than a reset, so
/// re-running provisioning must not clear notification settings.
fn upsert_identity(state: &SesState, identity: &str) {
    if state.identities.contains_key(identity) {
        return;
    }
    let domain = is_domain(identity);
    state.identities.insert(
        identity.to_string(),
        EmailIdentity {
            identity: identity.to_string(),
            // AWSim auto-verifies, matching what the v2 path already
            // does: a local run has no mailbox to click a link in.
            verified: true,
            identity_type: if domain { "DOMAIN" } else { "EMAIL_ADDRESS" }.to_string(),
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
            bounce_topic: None,
            complaint_topic: None,
            delivery_topic: None,
            forwarding_enabled: true,
            bounce_headers_included: false,
            complaint_headers_included: false,
            delivery_headers_included: false,
        },
    );
    info!(identity = %identity, "SES: verified identity");
}

// --- Identities --------------------------------------------------------------

/// VerifyEmailIdentity, and the deprecated VerifyEmailAddress alias.
pub fn verify_email_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let address = require_str(input, "EmailAddress")?;
    if is_domain(address) {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("`{address}` is not a valid email address"),
        ));
    }
    upsert_identity(state, address);
    Ok(json!({}))
}

pub fn verify_domain_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let domain = require_str(input, "Domain")?;
    if !is_domain(domain) {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("`{domain}` is not a valid domain"),
        ));
    }
    upsert_identity(state, domain);
    Ok(json!({ "VerificationToken": verification_token(domain) }))
}

pub fn list_identities(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let wanted = input.get("IdentityType").and_then(Value::as_str);
    let mut names: Vec<String> = state
        .identities
        .iter()
        .filter(|e| wanted.is_none_or(|t| t == e.identity_type))
        .map(|e| e.identity.clone())
        .collect();
    names.sort();
    Ok(json!({ "Identities": { "member": names } }))
}

/// ListVerifiedEmailAddresses. Deprecated in favour of ListIdentities,
/// but still what older tooling calls.
pub fn list_verified_email_addresses(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut names: Vec<String> = state
        .identities
        .iter()
        .filter(|e| e.verified && e.identity_type == "EMAIL_ADDRESS")
        .map(|e| e.identity.clone())
        .collect();
    names.sort();
    Ok(json!({ "VerifiedEmailAddresses": { "member": names } }))
}

/// DeleteIdentity, and the deprecated DeleteVerifiedEmailAddress alias.
///
/// AWS answers success whether or not the identity existed, so this
/// stays idempotent rather than reporting NotFound.
pub fn delete_identity(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input
        .get("Identity")
        .or_else(|| input.get("EmailAddress"))
        .and_then(Value::as_str)
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Identity is required"))?;
    state.identities.remove(identity);
    state.identity_policies.remove(identity);
    Ok(json!({}))
}

pub fn get_identity_verification_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let attrs = string_list(input, "Identities")
        .into_iter()
        .filter_map(|identity| {
            let entry = state.identities.get(&identity)?;
            let mut value = json!({
                "VerificationStatus": if entry.verified { "Success" } else { "Pending" },
            });
            if entry.identity_type == "DOMAIN" {
                value["VerificationToken"] = json!(verification_token(&identity));
            }
            Some((identity, value))
        })
        .collect::<Vec<_>>();
    Ok(json!({ "VerificationAttributes": entries(attrs) }))
}

// --- Identity notifications --------------------------------------------------

fn notification_field<'a>(
    entry: &'a mut EmailIdentity,
    notification_type: &str,
) -> Result<&'a mut Option<String>, AwsError> {
    match notification_type {
        "Bounce" => Ok(&mut entry.bounce_topic),
        "Complaint" => Ok(&mut entry.complaint_topic),
        "Delivery" => Ok(&mut entry.delivery_topic),
        other => Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("NotificationType `{other}` must be Bounce, Complaint, or Delivery"),
        )),
    }
}

pub fn set_identity_notification_topic(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let notification_type = require_str(input, "NotificationType")?;
    // Omitting SnsTopic clears the topic; that is how AWS turns a
    // notification back off.
    let topic = input.get("SnsTopic").and_then(Value::as_str);
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    *notification_field(&mut entry, notification_type)? = topic.map(str::to_string);
    Ok(json!({}))
}

pub fn set_identity_feedback_forwarding_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let enabled = parse_bool(input, "ForwardingEnabled")?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    entry.forwarding_enabled = enabled;
    Ok(json!({}))
}

pub fn set_identity_headers_in_notifications_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let notification_type = require_str(input, "NotificationType")?;
    let enabled = parse_bool(input, "Enabled")?;
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    match notification_type {
        "Bounce" => entry.bounce_headers_included = enabled,
        "Complaint" => entry.complaint_headers_included = enabled,
        "Delivery" => entry.delivery_headers_included = enabled,
        other => {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!("NotificationType `{other}` must be Bounce, Complaint, or Delivery"),
            ));
        }
    }
    Ok(json!({}))
}

pub fn get_identity_notification_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let attrs = string_list(input, "Identities")
        .into_iter()
        .filter_map(|identity| {
            let e = state.identities.get(&identity)?;
            let value = json!({
                "BounceTopic": e.bounce_topic.clone().unwrap_or_default(),
                "ComplaintTopic": e.complaint_topic.clone().unwrap_or_default(),
                "DeliveryTopic": e.delivery_topic.clone().unwrap_or_default(),
                "ForwardingEnabled": e.forwarding_enabled,
                "HeadersInBounceNotificationsEnabled": e.bounce_headers_included,
                "HeadersInComplaintNotificationsEnabled": e.complaint_headers_included,
                "HeadersInDeliveryNotificationsEnabled": e.delivery_headers_included,
            });
            Some((identity, value))
        })
        .collect::<Vec<_>>();
    Ok(json!({ "NotificationAttributes": entries(attrs) }))
}

// --- MAIL FROM ---------------------------------------------------------------

pub fn set_identity_mail_from_domain(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let domain = input.get("MailFromDomain").and_then(Value::as_str);
    let behavior = input
        .get("BehaviorOnMXFailure")
        .and_then(Value::as_str)
        .unwrap_or("UseDefaultValue");
    if !matches!(behavior, "UseDefaultValue" | "RejectMessage") {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("BehaviorOnMXFailure `{behavior}` must be UseDefaultValue or RejectMessage"),
        ));
    }
    let mut entry = state.identities.get_mut(identity).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("Identity not found: {identity}"),
        )
    })?;
    entry.mail_from_domain = domain.map(str::to_string);
    entry.mail_from_behavior_on_mx_failure = domain.map(|_| behavior.to_string());
    Ok(json!({}))
}

pub fn get_identity_mail_from_domain_attributes(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let attrs = string_list(input, "Identities")
        .into_iter()
        .filter_map(|identity| {
            let e = state.identities.get(&identity)?;
            let domain = e.mail_from_domain.clone()?;
            let value = json!({
                "MailFromDomain": domain,
                // AWSim cannot check MX records, so a configured domain
                // is reported as verified rather than left Pending.
                "MailFromDomainStatus": "Success",
                "BehaviorOnMXFailure": e
                    .mail_from_behavior_on_mx_failure
                    .clone()
                    .unwrap_or_else(|| "UseDefaultValue".to_string()),
            });
            Some((identity, value))
        })
        .collect::<Vec<_>>();
    Ok(json!({ "MailFromDomainAttributes": entries(attrs) }))
}

// --- Identity policies -------------------------------------------------------

pub fn put_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let policy_name = require_str(input, "PolicyName")?;
    let policy = require_str(input, "Policy")?;
    serde_json::from_str::<Value>(policy).map_err(|e| {
        AwsError::bad_request("InvalidPolicy", format!("Policy is not valid JSON: {e}"))
    })?;
    state
        .identity_policies
        .entry(identity.to_string())
        .or_default()
        .insert(policy_name.to_string(), policy.to_string());
    Ok(json!({}))
}

pub fn delete_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let policy_name = require_str(input, "PolicyName")?;
    if let Some(mut policies) = state.identity_policies.get_mut(identity) {
        policies.remove(policy_name);
    }
    Ok(json!({}))
}

pub fn list_identity_policies(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let mut names: Vec<String> = state
        .identity_policies
        .get(identity)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    Ok(json!({ "PolicyNames": { "member": names } }))
}

pub fn get_identity_policies(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = require_str(input, "Identity")?;
    let wanted = string_list(input, "PolicyNames");
    let stored: HashMap<String, String> = state
        .identity_policies
        .get(identity)
        .map(|p| p.clone())
        .unwrap_or_default();
    let selected = stored
        .into_iter()
        .filter(|(name, _)| wanted.is_empty() || wanted.contains(name))
        .map(|(name, policy)| (name, Value::String(policy)))
        .collect::<Vec<_>>();
    Ok(json!({ "Policies": entries(selected) }))
}

// --- Templates ---------------------------------------------------------------

fn template_from_input(input: &Value) -> Result<EmailTemplate, AwsError> {
    let template = input
        .get("Template")
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Template is required"))?;
    let name = require_str(template, "TemplateName")?;
    Ok(EmailTemplate {
        name: name.to_string(),
        subject: template
            .get("SubjectPart")
            .and_then(Value::as_str)
            .map(String::from),
        html: template
            .get("HtmlPart")
            .and_then(Value::as_str)
            .map(String::from),
        text: template
            .get("TextPart")
            .and_then(Value::as_str)
            .map(String::from),
        created_at: now_epoch(),
    })
}

pub fn create_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let template = template_from_input(input)?;
    if state.templates.contains_key(&template.name) {
        return Err(AwsError::conflict(
            "AlreadyExists",
            format!("Template already exists: {}", template.name),
        ));
    }
    info!(template = %template.name, "SES: created template");
    state.templates.insert(template.name.clone(), template);
    Ok(json!({}))
}

pub fn update_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut template = template_from_input(input)?;
    let existing = state.templates.get(&template.name).ok_or_else(|| {
        AwsError::not_found(
            "TemplateDoesNotExist",
            format!("Template not found: {}", template.name),
        )
    })?;
    // An update keeps the original creation time; only the content moves.
    template.created_at = existing.created_at;
    drop(existing);
    state.templates.insert(template.name.clone(), template);
    Ok(json!({}))
}

pub fn get_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "TemplateName")?;
    let t = state.templates.get(name).ok_or_else(|| {
        AwsError::not_found(
            "TemplateDoesNotExist",
            format!("Template not found: {name}"),
        )
    })?;
    Ok(json!({
        "Template": {
            "TemplateName": t.name,
            "SubjectPart": t.subject,
            "HtmlPart": t.html,
            "TextPart": t.text,
        }
    }))
}

pub fn delete_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "TemplateName")?;
    // AWS answers success for a template that was never there.
    state.templates.remove(name);
    Ok(json!({}))
}

pub fn list_templates(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut items: Vec<(String, u64)> = state
        .templates
        .iter()
        .map(|e| (e.name.clone(), e.created_at))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let metadata: Vec<Value> = items
        .into_iter()
        .map(|(name, created)| json!({ "Name": name, "CreatedTimestamp": created }))
        .collect();
    Ok(json!({ "TemplatesMetadata": { "member": metadata } }))
}

pub fn test_render_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "TemplateName")?;
    let data: Value = serde_json::from_str(input["TemplateData"].as_str().unwrap_or("{}"))
        .map_err(|e| {
            AwsError::bad_request(
                "InvalidRenderingParameter",
                format!("TemplateData is not valid JSON: {e}"),
            )
        })?;
    let t = state.templates.get(name).ok_or_else(|| {
        AwsError::not_found(
            "TemplateDoesNotExist",
            format!("Template not found: {name}"),
        )
    })?;
    let body = t.html.as_deref().or(t.text.as_deref()).unwrap_or("");
    let subject = t.subject.as_deref().unwrap_or("");
    let rendered = format!(
        "Subject: {}\n\n{}",
        render_template(subject, &data),
        render_template(body, &data)
    );
    Ok(json!({ "RenderedTemplate": rendered }))
}

// --- Account -----------------------------------------------------------------

fn sent_count(state: &SesState) -> u64 {
    state
        .sqlite()
        .and_then(|store| store.total_rows().ok())
        .unwrap_or(0)
}

pub fn get_send_quota(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // A production AWS account, not the sandbox: local runs should not
    // trip a quota that only exists to gate real outbound mail.
    Ok(json!({
        "Max24HourSend": 50000.0,
        "MaxSendRate": 14.0,
        "SentLast24Hours": sent_count(state) as f64,
    }))
}

pub fn get_send_statistics(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    // One aggregate data point. AWSim never bounces or rejects, so the
    // only non-zero counter is delivery attempts.
    Ok(json!({
        "SendDataPoints": { "member": [{
            "Timestamp": now_epoch(),
            "DeliveryAttempts": sent_count(state),
            "Bounces": 0,
            "Complaints": 0,
            "Rejects": 0,
        }]}
    }))
}

pub fn get_account_sending_enabled(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let enabled = state
        .account_sending_enabled
        .lock()
        .map(|g| g.unwrap_or(true))
        .unwrap_or(true);
    Ok(json!({ "Enabled": enabled }))
}

pub fn update_account_sending_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let enabled = parse_bool(input, "Enabled")?;
    let mut guard = state
        .account_sending_enabled
        .lock()
        .map_err(|_| AwsError::internal("Failed to acquire the account sending lock"))?;
    *guard = Some(enabled);
    Ok(json!({}))
}

// --- Receipt filters ---------------------------------------------------------

pub fn create_receipt_filter(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter = input
        .get("Filter")
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Filter is required"))?;
    let name = require_str(filter, "Name")?;
    let ip_filter = filter
        .get("IpFilter")
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Filter.IpFilter is required"))?;
    let policy = require_str(ip_filter, "Policy")?;
    if !matches!(policy, "Allow" | "Block") {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!("Policy `{policy}` must be Allow or Block"),
        ));
    }
    let cidr = require_str(ip_filter, "Cidr")?;
    if state.receipt_filters.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExists",
            format!("Receipt filter already exists: {name}"),
        ));
    }
    state.receipt_filters.insert(
        name.to_string(),
        ReceiptFilter {
            name: name.to_string(),
            policy: policy.to_string(),
            cidr: cidr.to_string(),
        },
    );
    Ok(json!({}))
}

pub fn delete_receipt_filter(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "FilterName")?;
    state.receipt_filters.remove(name);
    Ok(json!({}))
}

pub fn list_receipt_filters(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut filters: Vec<Value> = state
        .receipt_filters
        .iter()
        .map(|e| {
            json!({
                "Name": e.name,
                "IpFilter": { "Policy": e.policy, "Cidr": e.cidr },
            })
        })
        .collect();
    filters.sort_by(|a, b| a["Name"].as_str().cmp(&b["Name"].as_str()));
    Ok(json!({ "Filters": { "member": filters } }))
}

// --- Configuration sets ------------------------------------------------------

pub fn describe_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "ConfigurationSetName")?;
    let cs = state.configuration_sets.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ConfigurationSetDoesNotExist",
            format!("Configuration set not found: {name}"),
        )
    })?;

    let destinations: Vec<Value> = cs
        .event_destinations
        .iter()
        .map(|d| {
            let mut obj = Map::new();
            obj.insert("Name".to_string(), json!(d.name));
            obj.insert("Enabled".to_string(), json!(d.enabled));
            obj.insert(
                "MatchingEventTypes".to_string(),
                json!({ "member": d.matching_event_types }),
            );
            if let Some(arn) = &d.sns_topic_arn {
                obj.insert("SNSDestination".to_string(), json!({ "TopicARN": arn }));
            }
            if let Some(arn) = &d.firehose_delivery_stream_arn {
                obj.insert(
                    "KinesisFirehoseDestination".to_string(),
                    json!({ "DeliveryStreamARN": arn }),
                );
            }
            if !d.cloudwatch_dimensions.is_empty() {
                obj.insert(
                    "CloudWatchDestination".to_string(),
                    json!({ "DimensionConfigurations": { "member": d.cloudwatch_dimensions } }),
                );
            }
            Value::Object(obj)
        })
        .collect();

    let mut out = json!({
        "ConfigurationSet": { "Name": cs.name },
        "EventDestinations": { "member": destinations },
        "ReputationOptions": {
            "SendingEnabled": cs.sending_enabled,
            "ReputationMetricsEnabled": cs.reputation_metrics_enabled,
        },
    });
    if let Some(policy) = &cs.tls_policy {
        out["DeliveryOptions"] = json!({ "TlsPolicy": policy });
    }
    if let Some(domain) = &cs.custom_redirect_domain {
        out["TrackingOptions"] = json!({ "CustomRedirectDomain": domain });
    }
    if let Some(ts) = cs.reputation_last_fresh_start {
        out["ReputationOptions"]["LastFreshStart"] = json!(ts);
    }
    Ok(out)
}

// --- Receipt rule sets -------------------------------------------------------

pub fn clone_receipt_rule_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "RuleSetName")?;
    let original_name = require_str(input, "OriginalRuleSetName")?;
    if state.receipt_rule_sets.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExists",
            format!("Rule set already exists: {name}"),
        ));
    }
    let original = state.receipt_rule_sets.get(original_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {original_name}"),
        )
    })?;
    let mut clone = original.clone();
    drop(original);
    clone.name = name.to_string();
    clone.created_at = now_epoch();
    state.receipt_rule_sets.insert(name.to_string(), clone);
    Ok(json!({}))
}

/// SetReceiptRulePosition. Move a rule to sit directly after `After`,
/// or to the front of the set when `After` is omitted.
pub fn set_receipt_rule_position(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let set_name = require_str(input, "RuleSetName")?;
    let rule_name = require_str(input, "RuleName")?;
    let after = input.get("After").and_then(Value::as_str);

    let mut set = state.receipt_rule_sets.get_mut(set_name).ok_or_else(|| {
        AwsError::not_found(
            "RuleSetDoesNotExist",
            format!("Rule set does not exist: {set_name}"),
        )
    })?;
    let from = set
        .rules
        .iter()
        .position(|r| r.name == rule_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "RuleDoesNotExist",
                format!("Rule does not exist: {rule_name}"),
            )
        })?;
    let rule = set.rules.remove(from);
    // Resolve the anchor after the removal so its index is the one the
    // rule will actually be inserted behind.
    let to = match after {
        Some(anchor) => match set.rules.iter().position(|r| r.name == anchor) {
            Some(pos) => pos + 1,
            None => {
                set.rules.insert(from, rule);
                return Err(AwsError::not_found(
                    "RuleDoesNotExist",
                    format!("Rule does not exist: {anchor}"),
                ));
            }
        },
        None => 0,
    };
    set.rules.insert(to, rule);
    Ok(json!({}))
}

/// ListConfigurationSets. The classic API returns a list of structures
/// with a `Name`, where v2 returns a list of bare names, so this is the
/// one shared operation the two APIs cannot answer from one shape.
pub fn list_configuration_sets(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut names: Vec<String> = state
        .configuration_sets
        .iter()
        .map(|e| e.key().clone())
        .collect();
    names.sort();
    let sets: Vec<Value> = names.into_iter().map(|n| json!({ "Name": n })).collect();
    Ok(json!({ "ConfigurationSets": { "member": sets } }))
}

fn configuration_set_mut<'a>(
    state: &'a SesState,
    input: &Value,
) -> Result<dashmap::mapref::one::RefMut<'a, String, crate::state::ConfigurationSet>, AwsError> {
    let name = require_str(input, "ConfigurationSetName")?;
    state.configuration_sets.get_mut(name).ok_or_else(|| {
        AwsError::not_found(
            "ConfigurationSetDoesNotExist",
            format!("Configuration set not found: {name}"),
        )
    })
}

/// Create/Update/PutConfigurationSetTrackingOptions. All three are the
/// same write; only the surrounding API differs, with v2 passing the
/// domain flat and the classic API nesting it under `TrackingOptions`.
pub fn put_configuration_set_tracking_options(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let tracking = &input["TrackingOptions"];
    let domain = input["CustomRedirectDomain"]
        .as_str()
        .or_else(|| tracking["CustomRedirectDomain"].as_str());
    let https_policy = input["HttpsPolicy"]
        .as_str()
        .or_else(|| tracking["HttpsPolicy"].as_str());
    let mut cs = configuration_set_mut(state, input)?;
    cs.custom_redirect_domain = domain.map(str::to_string);
    cs.tracking_https_policy = https_policy.map(str::to_string);
    Ok(json!({}))
}

pub fn delete_configuration_set_tracking_options(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let mut cs = configuration_set_mut(state, input)?;
    cs.custom_redirect_domain = None;
    cs.tracking_https_policy = None;
    Ok(json!({}))
}

pub fn update_configuration_set_sending_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let enabled = parse_bool(input, "Enabled")?;
    configuration_set_mut(state, input)?.sending_enabled = enabled;
    Ok(json!({}))
}

pub fn update_configuration_set_reputation_metrics_enabled(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let enabled = parse_bool(input, "Enabled")?;
    let mut cs = configuration_set_mut(state, input)?;
    // AWS stamps LastFreshStart the moment metrics are turned on, which
    // is what scopes the reputation window shown in the console.
    if enabled && !cs.reputation_metrics_enabled {
        cs.reputation_last_fresh_start = Some(now_epoch());
    }
    cs.reputation_metrics_enabled = enabled;
    Ok(json!({}))
}

/// The query protocol carries booleans as the strings "true"/"false",
/// while an in-process caller passes a real bool.
fn parse_bool(input: &Value, key: &str) -> Result<bool, AwsError> {
    match input.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("true") => Ok(true),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err(AwsError::bad_request(
            "InvalidParameter",
            format!("{key} is required and must be a boolean"),
        )),
    }
}
