use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{SentEmail, SesState};

/// Current epoch in seconds.
fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Publish `ses:EmailEvent` records onto the internal event bus for
/// every enabled event destination whose `MatchingEventTypes` covers a
/// just-occurred event. A synthetic accept-and-deliver maps to the
/// `SEND` and `DELIVERY` event types; the binary's event router fans
/// each record out to the configured SNS topic, Firehose stream, or
/// CloudWatch namespace named in the detail.
///
/// No-op when the request carries no bus (unit tests, internal delivery
/// contexts that set `event_bus: None` to avoid feedback loops) or when
/// the send named no configuration set.
pub(crate) fn emit_send_events(
    ctx: &RequestContext,
    state: &SesState,
    cs_name: Option<&str>,
    email: &SentEmail,
) {
    let Some(bus) = ctx.event_bus.as_ref() else {
        return;
    };
    let Some(name) = cs_name else {
        return;
    };
    let Some(cs) = state.configuration_sets.get(name) else {
        return;
    };
    let mail = json!({
        "messageId": email.message_id,
        "source": email.from,
        "destination": email.to,
        "tags": email
            .tags
            .iter()
            .map(|(k, v)| json!({ "name": k, "value": v }))
            .collect::<Vec<_>>(),
    });
    for event_type in ["SEND", "DELIVERY"] {
        for d in &cs.event_destinations {
            if !d.enabled || !d.matching_event_types.iter().any(|t| t == event_type) {
                continue;
            }
            let destination = if let Some(arn) = &d.sns_topic_arn {
                json!({ "kind": "sns", "arn": arn })
            } else if let Some(arn) = &d.firehose_delivery_stream_arn {
                json!({ "kind": "firehose", "arn": arn })
            } else {
                json!({ "kind": "cloudwatch", "dimensions": d.cloudwatch_dimensions })
            };
            bus.publish(awsim_core::events::InternalEvent {
                source: "ses".into(),
                event_type: "ses:EmailEvent".into(),
                region: ctx.region.clone(),
                account_id: ctx.account_id.clone(),
                detail: json!({
                    "configurationSetName": name,
                    "destinationName": d.name,
                    "eventType": event_type,
                    "destination": destination,
                    "mail": mail,
                }),
            });
        }
    }
}

/// Returns an error when the recipient is opted out — either globally
/// via `UnsubscribeAll`, or for the named topic via a `TopicPreferences`
/// entry whose `SubscriptionStatus` is `OPT_OUT`. Recipients with no
/// contact record default to opted-in.
fn check_topic_subscription(
    state: &SesState,
    list_name: &str,
    topic_name: Option<&str>,
    recipient: &str,
) -> Result<(), AwsError> {
    let key = format!("{list_name}#{recipient}");
    let Some(contact) = state.contacts.get(&key) else {
        return Ok(());
    };
    if contact.unsubscribe_all {
        return Err(AwsError::bad_request(
            "MessageRejected",
            format!("Recipient {recipient} has unsubscribed from list {list_name}"),
        ));
    }
    if let Some(topic) = topic_name {
        for pref in &contact.topic_preferences {
            if pref.get("TopicName").and_then(|v| v.as_str()) == Some(topic)
                && pref.get("SubscriptionStatus").and_then(|v| v.as_str()) == Some("OPT_OUT")
            {
                return Err(AwsError::bad_request(
                    "MessageRejected",
                    format!(
                        "Recipient {recipient} has opted out of topic {topic} on list {list_name}"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Substitute `{{key}}` placeholders in `text` with stringified values
/// from `data`. Unknown keys collapse to an empty string, matching SES's
/// behavior when TemplateData omits a referenced variable.
fn render_template(text: &str, data: &Value) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find("}}") {
            let key = rest[..end].trim();
            let value = data.get(key).map(value_to_plain).unwrap_or_default();
            out.push_str(&value);
            rest = &rest[end + 2..];
        } else {
            out.push_str("{{");
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Coerce a JSON scalar to its plain string form (avoiding the quotes
/// that `Value::to_string` adds around strings).
fn value_to_plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// SendTemplatedEmail (SES v1) — render a stored template against
/// TemplateData and send to the recipients. Honors Cc/Bcc/ReplyTo +
/// ConfigurationSetName + Tags so the persisted row carries the same
/// metadata as SendEmail. Accepts v1 (`Source`) and v2
/// (`FromEmailAddress`) sender keys interchangeably.
pub fn send_templated_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let from = input["Source"]
        .as_str()
        .or_else(|| input["FromEmailAddress"].as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Source is required"))?
        .to_string();

    let destination = &input["Destination"];
    let to: Vec<String> = address_list(&destination["ToAddresses"]);
    let cc: Vec<String> = address_list(&destination["CcAddresses"]);
    let bcc: Vec<String> = address_list(&destination["BccAddresses"]);
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "At least one recipient address is required",
        ));
    }

    let reply_to: Vec<String> = address_list(&input["ReplyToAddresses"]);
    let template_name = input["Template"]
        .as_str()
        .or_else(|| input["TemplateName"].as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Template is required"))?;
    let template = state.templates.get(template_name).ok_or_else(|| {
        AwsError::not_found(
            "TemplateDoesNotExist",
            format!("Template not found: {template_name}"),
        )
    })?;

    let data_str = input["TemplateData"].as_str().unwrap_or("{}");
    let data: Value = serde_json::from_str(data_str).map_err(|_| {
        AwsError::bad_request(
            "InvalidParameter",
            "TemplateData must be a JSON object string",
        )
    })?;
    let subject = template
        .subject
        .as_deref()
        .map(|s| render_template(s, &data));
    let body_text = template.text.as_deref().map(|s| render_template(s, &data));
    let body_html = template.html.as_deref().map(|s| render_template(s, &data));
    let configuration_set_name = input["ConfigurationSetName"].as_str().map(str::to_string);
    let tags = parse_email_tags(input.get("Tags").or_else(|| input.get("EmailTags")));
    enforce_configuration_set(state, configuration_set_name.as_deref(), input)?;

    let message_id = Uuid::new_v4().to_string();
    let email = SentEmail {
        message_id: message_id.clone(),
        from,
        to,
        cc,
        bcc,
        reply_to,
        subject,
        body_text,
        body_html,
        raw: None,
        sent_at: now_epoch(),
        configuration_set_name,
        tags,
    };

    info!(message_id = %message_id, "SES: templated email sent");
    if let Some(store) = state.sqlite() {
        store.put_email(&ctx.account_id, &ctx.region, &email)?;
    }
    emit_send_events(ctx, state, email.configuration_set_name.as_deref(), &email);
    Ok(json!({ "MessageId": message_id }))
}

/// SendBulkTemplatedEmail (SES v1) — render a single template to many
/// destinations. Each `Destinations[]` entry can carry its own
/// `ReplacementTemplateData` that overrides the request-level
/// `DefaultTemplateData`. The response surfaces a per-destination
/// status row, matching the AWS shape `{ "Status": [{...}] }`.
pub fn send_bulk_templated_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let from = input["Source"]
        .as_str()
        .or_else(|| input["FromEmailAddress"].as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Source is required"))?
        .to_string();

    let template_name = input["Template"]
        .as_str()
        .or_else(|| input["TemplateName"].as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Template is required"))?;
    let template = state.templates.get(template_name).ok_or_else(|| {
        AwsError::not_found(
            "TemplateDoesNotExist",
            format!("Template not found: {template_name}"),
        )
    })?;

    let default_data: Value = input["DefaultTemplateData"]
        .as_str()
        .map(|s| serde_json::from_str(s).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    let configuration_set_name = input["ConfigurationSetName"].as_str().map(str::to_string);
    enforce_configuration_set(state, configuration_set_name.as_deref(), input)?;
    let reply_to = address_list(&input["ReplyToAddresses"]);
    let request_tags = parse_email_tags(input.get("DefaultTags").or_else(|| input.get("Tags")));

    let destinations = input["Destinations"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut statuses: Vec<Value> = Vec::with_capacity(destinations.len());
    for dest in destinations {
        let to = address_list(&dest["Destination"]["ToAddresses"]);
        let cc = address_list(&dest["Destination"]["CcAddresses"]);
        let bcc = address_list(&dest["Destination"]["BccAddresses"]);
        if to.is_empty() && cc.is_empty() && bcc.is_empty() {
            statuses.push(json!({
                "Status": "MessageRejected",
                "Error": "At least one recipient address is required",
            }));
            continue;
        }

        let replacement_data: Value = dest["ReplacementTemplateData"]
            .as_str()
            .map(|s| serde_json::from_str(s).unwrap_or(Value::Null))
            .unwrap_or(Value::Null);
        let merged = merge_template_data(&default_data, &replacement_data);

        let subject = template
            .subject
            .as_deref()
            .map(|s| render_template(s, &merged));
        let body_text = template
            .text
            .as_deref()
            .map(|s| render_template(s, &merged));
        let body_html = template
            .html
            .as_deref()
            .map(|s| render_template(s, &merged));

        // Merge ReplacementTags over the request-level DefaultTags.
        let mut tags = request_tags.clone();
        let replacement_tags = parse_email_tags(Some(&dest["ReplacementTags"]));
        for (k, v) in replacement_tags {
            if let Some(existing) = tags.iter_mut().find(|(name, _)| *name == k) {
                existing.1 = v;
            } else {
                tags.push((k, v));
            }
        }

        let message_id = Uuid::new_v4().to_string();
        let email = SentEmail {
            message_id: message_id.clone(),
            from: from.clone(),
            to,
            cc,
            bcc,
            reply_to: reply_to.clone(),
            subject,
            body_text,
            body_html,
            raw: None,
            sent_at: now_epoch(),
            configuration_set_name: configuration_set_name.clone(),
            tags,
        };

        if let Some(store) = state.sqlite() {
            store.put_email(&ctx.account_id, &ctx.region, &email)?;
        }
        emit_send_events(ctx, state, email.configuration_set_name.as_deref(), &email);
        statuses.push(json!({ "Status": "Success", "MessageId": message_id }));
    }

    Ok(json!({ "Status": statuses }))
}

/// Merge per-destination `ReplacementTemplateData` over the request's
/// `DefaultTemplateData`. AWS treats both as flat JSON objects of
/// scalar values; replacement keys win when both sides define the same
/// name.
fn merge_template_data(default: &Value, replacement: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = default.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), v.clone());
        }
    }
    if let Some(obj) = replacement.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// SendRawEmail (SES v1) — accept an RFC 2822 message and route it.
/// Parses the raw MIME headers to pull out subject + recipients when
/// the caller doesn't supply Destinations explicitly, and persists
/// ConfigurationSetName / Tags from the request. `Source` falls back to
/// the message's `From:` header. `ReturnPath` and `SourceArn` are
/// recorded for downstream visibility but not used by the simulator.
/// `RawMessage.Data` may be base64-encoded; we tolerate both forms.
pub fn send_raw_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let raw_data = input["RawMessage"]["Data"]
        .as_str()
        .or_else(|| input["RawMessage"]["data"].as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "RawMessage.Data is required"))?;
    let raw = decode_raw_message(raw_data);
    let headers = parse_rfc2822_headers(&raw);

    let from = input["Source"]
        .as_str()
        .or_else(|| input["FromEmailAddress"].as_str())
        .map(str::to_string)
        .or_else(|| {
            // Strip the optional display-name wrapper when pulling From
            // from the parsed message ("Alice <alice@example.com>" →
            // "alice@example.com").
            headers
                .get("from")
                .map(|h| strip_display_name(h).to_string())
        })
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameter",
                "Source is required (or supply a From: header)",
            )
        })?;

    // AWS treats Destinations[] as the union of To/Cc/Bcc when the
    // caller passes them explicitly; otherwise pull from headers.
    let explicit_destinations = address_list(&input["Destinations"]);
    let (to, cc, bcc) = if explicit_destinations.is_empty() {
        (
            address_list_from_header(headers.get("to")),
            address_list_from_header(headers.get("cc")),
            address_list_from_header(headers.get("bcc")),
        )
    } else {
        (explicit_destinations, Vec::new(), Vec::new())
    };
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "At least one recipient address is required",
        ));
    }

    let reply_to = address_list_from_header(headers.get("reply-to"));
    let subject = headers.get("subject").cloned();
    let configuration_set_name = input["ConfigurationSetName"].as_str().map(str::to_string);
    let tags = parse_email_tags(input.get("Tags").or_else(|| input.get("EmailTags")));
    enforce_configuration_set(state, configuration_set_name.as_deref(), input)?;
    // ReturnPath / SourceArn: AWS treats these as identity-routing hints
    // (cross-account sending, bounce destination). We log them for
    // observability; downstream simulator paths don't need them yet.
    let return_path = input["ReturnPath"]
        .as_str()
        .or_else(|| headers.get("return-path").map(String::as_str));
    let source_arn = input["SourceArn"].as_str();
    if return_path.is_some() || source_arn.is_some() {
        info!(
            return_path = ?return_path,
            source_arn = ?source_arn,
            "SES: SendRawEmail received identity-routing hints"
        );
    }

    let message_id = Uuid::new_v4().to_string();
    let email = SentEmail {
        message_id: message_id.clone(),
        from,
        to,
        cc,
        bcc,
        reply_to,
        subject,
        body_text: None,
        body_html: None,
        raw: Some(raw),
        sent_at: now_epoch(),
        configuration_set_name,
        tags,
    };

    info!(message_id = %message_id, "SES: raw email sent");
    if let Some(store) = state.sqlite() {
        store.put_email(&ctx.account_id, &ctx.region, &email)?;
    }
    emit_send_events(ctx, state, email.configuration_set_name.as_deref(), &email);
    Ok(json!({ "MessageId": message_id }))
}

/// Decode a RawMessage.Data field. AWS accepts both base64-encoded and
/// already-decoded payloads; we attempt base64 first, then fall back to
/// the original string when decoding fails or yields invalid UTF-8.
fn decode_raw_message(raw: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    if let Ok(bytes) = STANDARD.decode(raw.trim())
        && let Ok(s) = String::from_utf8(bytes)
    {
        return s;
    }
    raw.to_string()
}

/// Parse RFC 2822 headers from the start of a raw message. Continuation
/// lines (leading whitespace) are folded into the previous header's
/// value. Header names are normalized to lowercase for lookup.
fn parse_rfc2822_headers(raw: &str) -> std::collections::HashMap<String, String> {
    let mut out: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_key: Option<String> = None;
    for line in raw.lines() {
        if line.is_empty() {
            break; // end of headers
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(k) = &last_key
                && let Some(v) = out.get_mut(k)
            {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            out.insert(key.clone(), value.trim().to_string());
            last_key = Some(key);
        }
    }
    out
}

/// Split a comma-separated address header into individual addresses,
/// stripping display names and the angle-bracket wrapping AWS expects.
fn address_list_from_header(header: Option<&String>) -> Vec<String> {
    let Some(h) = header else { return Vec::new() };
    h.split(',')
        .filter_map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(strip_display_name(trimmed).to_string())
        })
        .collect()
}

fn strip_display_name(raw: &str) -> &str {
    if let Some(start) = raw.rfind('<')
        && let Some(end) = raw.rfind('>')
        && start < end
    {
        return &raw[start + 1..end];
    }
    raw
}

fn address_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse EmailTags input ([{Name, Value}, ...]) into a flat name/value
/// vector. Entries missing either field are dropped. Empty input yields
/// an empty vector.
fn parse_email_tags(tags: Option<&Value>) -> Vec<(String, String)> {
    tags.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("Name").and_then(|v| v.as_str())?;
                    let value = t.get("Value").and_then(|v| v.as_str())?;
                    Some((name.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Apply per-send guards from the named configuration set:
///
/// - Missing set yields `ConfigurationSetDoesNotExist` (404).
/// - `SendingOptions.SendingEnabled = false` yields
///   `AccountSendingPausedException`, matching AWS when sends are paused.
/// - `DeliveryOptions.TlsPolicy = REQUIRE` paired with an
///   `aws-ses-disable-tls` EmailTag yields `MessageRejected`.
///
/// Reputation metrics are a passive signal (no rejection); we only
/// persist them so dashboards can consume the field.
fn enforce_configuration_set(
    state: &SesState,
    name: Option<&str>,
    input: &Value,
) -> Result<(), AwsError> {
    let Some(name) = name else { return Ok(()) };
    let cs = state.configuration_sets.get(name).ok_or_else(|| {
        AwsError::not_found(
            "ConfigurationSetDoesNotExist",
            format!("Configuration set does not exist: {name}"),
        )
    })?;
    if !cs.sending_enabled {
        return Err(AwsError::bad_request(
            "AccountSendingPausedException",
            format!("ConfigurationSet '{name}' has sending disabled"),
        ));
    }
    if cs.tls_policy.as_deref() == Some("REQUIRE")
        && tags_signal_no_tls(input.get("EmailTags").or_else(|| input.get("Tags")))
    {
        return Err(AwsError::bad_request(
            "MessageRejected",
            "ConfigurationSet TlsPolicy is REQUIRE; recipient does not support TLS",
        ));
    }
    Ok(())
}

/// Returns true when EmailTags carry the `aws-ses-disable-tls` marker
/// with a truthy value, used by the simulator to model a recipient MTA
/// without TLS support.
fn tags_signal_no_tls(tags: Option<&Value>) -> bool {
    let Some(arr) = tags.and_then(|v| v.as_array()) else {
        return false;
    };
    arr.iter().any(|t| {
        t.get("Name").and_then(|v| v.as_str()) == Some("aws-ses-disable-tls")
            && matches!(
                t.get("Value").and_then(|v| v.as_str()),
                Some("true" | "1" | "yes")
            )
    })
}

/// Lightweight well-formedness check for an SES identity ARN supplied
/// as a routing hint (FromEmailAddressIdentityArn / ReturnPathArn). The
/// full cross-account parser lives in `lib.rs` and isn't reachable from
/// here, so we mirror its prefix requirement: a valid SES ARN starts
/// with `arn:aws:ses:`. Empty values are treated as absent (Ok).
fn validate_ses_identity_arn(field: &str, value: &str) -> Result<(), AwsError> {
    if value.is_empty() || value.starts_with("arn:aws:ses:") {
        return Ok(());
    }
    Err(AwsError::bad_request(
        "InvalidParameter",
        format!("{field} `{value}` is not a valid SES identity ARN."),
    ))
}

/// Reject a send when any To/Cc/Bcc recipient sits on the account-level
/// suppression list with a reason the account currently enforces. The
/// enabled reasons come from `account_suppression_attributes`
/// (PutAccountSuppressionAttributes); an empty or unset list enforces
/// nothing, matching AWS where suppression only applies to reasons the
/// account has opted into.
fn check_account_suppression(state: &SesState, recipient: &str) -> Result<(), AwsError> {
    let Some(entry) = state.suppressed_destinations.get(recipient) else {
        return Ok(());
    };
    let attrs = state.account_suppression_attributes.lock().unwrap();
    let Some(attrs) = attrs.as_ref() else {
        return Ok(());
    };
    let enforced = attrs["SuppressedReasons"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .any(|r| r.as_str() == Some(entry.reason.as_str()))
        })
        .unwrap_or(false);
    if enforced {
        return Err(AwsError::bad_request(
            "MessageRejected",
            format!(
                "Recipient {recipient} is on the account suppression list ({})",
                entry.reason
            ),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// SendEmail
// ---------------------------------------------------------------------------

pub fn send_email(
    state: &SesState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let from = input["FromEmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "FromEmailAddress is required"))?
        .to_string();

    // Destination
    let destination = &input["Destination"];
    let to: Vec<String> = destination["ToAddresses"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let cc: Vec<String> = destination["CcAddresses"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let bcc: Vec<String> = destination["BccAddresses"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "At least one recipient address is required",
        ));
    }

    let reply_to: Vec<String> = input["ReplyToAddresses"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let configuration_set_name = input["ConfigurationSetName"].as_str().map(str::to_string);

    // Identity-routing hints. AWS treats these as cross-account /
    // bounce-routing metadata; the simulator accepts them faithfully
    // (validating the ARN-typed ones) and logs them for observability
    // rather than inventing storage columns. Mirrors how SendRawEmail
    // records ReturnPath / SourceArn.
    let from_identity_arn = input["FromEmailAddressIdentityArn"].as_str();
    let return_path = input["ReturnPath"].as_str();
    let return_path_arn = input["ReturnPathArn"].as_str();
    let feedback_forwarding = input["FeedbackForwardingEmailAddress"].as_str();
    if let Some(arn) = from_identity_arn {
        validate_ses_identity_arn("FromEmailAddressIdentityArn", arn)?;
    }
    if let Some(arn) = return_path_arn {
        validate_ses_identity_arn("ReturnPathArn", arn)?;
    }
    if from_identity_arn.is_some()
        || return_path.is_some()
        || return_path_arn.is_some()
        || feedback_forwarding.is_some()
    {
        info!(
            from_identity_arn = ?from_identity_arn,
            return_path = ?return_path,
            return_path_arn = ?return_path_arn,
            feedback_forwarding = ?feedback_forwarding,
            "SES: SendEmail received identity-routing hints"
        );
    }

    // ListManagementOptions: when the caller scopes a send to a contact
    // list (and optionally a topic), each recipient is checked against
    // their subscription preferences. Unsubscribed contacts and OPT_OUT
    // topic preferences are refused before fan-out, matching AWS's
    // pre-send suppression behavior.
    let list_name = input["ListManagementOptions"]["ContactListName"]
        .as_str()
        .map(str::to_string);
    let topic_name = input["ListManagementOptions"]["TopicName"]
        .as_str()
        .map(str::to_string);
    if let Some(ref list) = list_name {
        for recipient in to.iter().chain(cc.iter()).chain(bcc.iter()) {
            check_topic_subscription(state, list, topic_name.as_deref(), recipient)?;
        }
    }

    // Account-level suppression: reject before fan-out when a recipient
    // is suppressed for a reason the account enforces.
    for recipient in to.iter().chain(cc.iter()).chain(bcc.iter()) {
        check_account_suppression(state, recipient)?;
    }

    enforce_configuration_set(state, configuration_set_name.as_deref(), input)?;

    // Content — Simple, Raw, or Templated. The Templated branch loads
    // the named template, parses the TemplateData JSON string, and
    // expands `{{var}}` placeholders within each part. AWS SES is
    // Handlebars-compatible; we cover the common substitution case.
    let content = &input["Content"];
    let (subject, body_text, body_html, raw) = if !content["Simple"].is_null() {
        let simple = &content["Simple"];
        let subject = simple["Subject"]["Data"].as_str().map(String::from);
        let body_text = simple["Body"]["Text"]["Data"].as_str().map(String::from);
        let body_html = simple["Body"]["Html"]["Data"].as_str().map(String::from);
        (subject, body_text, body_html, None)
    } else if !content["Raw"].is_null() {
        let raw_data = content["Raw"]["Data"].as_str().map(String::from);
        (None, None, None, raw_data)
    } else if !content["Templated"].is_null() {
        let templated = &content["Templated"];
        let template_name = templated["TemplateName"].as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameter",
                "Content.Templated requires TemplateName",
            )
        })?;
        let template = state.templates.get(template_name).ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Template not found: {template_name}"),
            )
        })?;
        let data_str = templated["TemplateData"].as_str().unwrap_or("{}");
        let data: Value = serde_json::from_str(data_str).map_err(|_| {
            AwsError::bad_request(
                "InvalidParameter",
                "Content.Templated.TemplateData must be a JSON object string",
            )
        })?;
        let subject = template
            .subject
            .as_deref()
            .map(|s| render_template(s, &data));
        let body_text = template.text.as_deref().map(|s| render_template(s, &data));
        let body_html = template.html.as_deref().map(|s| render_template(s, &data));
        (subject, body_text, body_html, None)
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Content must include Simple, Raw, or Templated",
        ));
    };

    let message_id = Uuid::new_v4().to_string();

    let tags = parse_email_tags(input.get("EmailTags"));

    let email = SentEmail {
        message_id: message_id.clone(),
        from,
        to,
        cc,
        bcc,
        reply_to,
        subject,
        body_text,
        body_html,
        raw,
        sent_at: now_epoch(),
        configuration_set_name,
        tags,
    };

    info!(message_id = %message_id, "SES: email sent");
    if let Some(store) = state.sqlite() {
        store.put_email(&ctx.account_id, &ctx.region, &email)?;
    }
    emit_send_events(ctx, state, email.configuration_set_name.as_deref(), &email);

    Ok(json!({ "MessageId": message_id }))
}

#[cfg(test)]
mod tls_policy_enforcement_tests {
    use super::*;
    use crate::operations::more::create_configuration_set;

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn send_input(cs_name: &str, no_tls: bool) -> Value {
        let mut tags = vec![json!({ "Name": "campaign", "Value": "spring" })];
        if no_tls {
            tags.push(json!({ "Name": "aws-ses-disable-tls", "Value": "true" }));
        }
        json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["recipient@example.com"] },
            "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } },
            "ConfigurationSetName": cs_name,
            "EmailTags": tags,
        })
    }

    #[test]
    fn require_policy_blocks_send_marked_plaintext_only() {
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
        let err = send_email(&state, &send_input("cs", true), &ctx()).unwrap_err();
        assert_eq!(err.code, "MessageRejected");
        assert!(err.message.contains("REQUIRE"));
    }

    #[test]
    fn require_policy_allows_send_without_disable_tag() {
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
        let resp = send_email(&state, &send_input("cs", false), &ctx()).unwrap();
        assert!(resp["MessageId"].is_string());
    }

    #[test]
    fn optional_policy_accepts_plaintext_only_send() {
        let state = SesState::default();
        create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "DeliveryOptions": { "TlsPolicy": "OPTIONAL" },
            }),
            &ctx(),
        )
        .unwrap();
        let resp = send_email(&state, &send_input("cs", true), &ctx()).unwrap();
        assert!(resp["MessageId"].is_string());
    }

    fn open_store() -> std::sync::Arc<crate::SqliteStore> {
        let path = std::env::temp_dir().join(format!("awsim-ses-cc-{}.db", uuid::Uuid::new_v4()));
        std::sync::Arc::new(crate::SqliteStore::open(path).unwrap())
    }

    #[test]
    fn send_bulk_templated_email_merges_replacement_data_per_destination() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        crate::operations::templates::create_email_template(
            &state,
            &json!({
                "TemplateName": "promo",
                "TemplateContent": {
                    "Subject": "Hi {{name}}",
                    "Text": "Code {{code}} for {{name}}.",
                }
            }),
            &ctx(),
        )
        .unwrap();

        let resp = send_bulk_templated_email(
            &state,
            &json!({
                "Source": "alice@example.com",
                "Template": "promo",
                "DefaultTemplateData": "{\"name\":\"friend\",\"code\":\"SAVE10\"}",
                "Destinations": [
                    {
                        "Destination": { "ToAddresses": ["a@example.com"] },
                        "ReplacementTemplateData": "{\"name\":\"Alex\"}"
                    },
                    {
                        "Destination": { "ToAddresses": ["b@example.com"] },
                        "ReplacementTemplateData": "{\"code\":\"VIP\"}"
                    }
                ]
            }),
            &ctx(),
        )
        .unwrap();

        let statuses = resp["Status"].as_array().unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|s| s["Status"] == "Success"));

        let rows = store.list_all().unwrap();
        assert_eq!(rows.len(), 2);
        let by_to: std::collections::HashMap<_, _> = rows
            .iter()
            .map(|r| (r.email.to[0].clone(), &r.email))
            .collect();
        assert_eq!(by_to["a@example.com"].subject.as_deref(), Some("Hi Alex"));
        assert_eq!(
            by_to["a@example.com"].body_text.as_deref(),
            Some("Code SAVE10 for Alex.")
        );
        assert_eq!(by_to["b@example.com"].subject.as_deref(), Some("Hi friend"));
        assert_eq!(
            by_to["b@example.com"].body_text.as_deref(),
            Some("Code VIP for friend.")
        );
    }

    #[test]
    fn send_bulk_templated_email_rejects_destination_without_recipients() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::templates::create_email_template(
            &state,
            &json!({ "TemplateName": "t", "TemplateContent": { "Subject": "x" } }),
            &ctx(),
        )
        .unwrap();
        let resp = send_bulk_templated_email(
            &state,
            &json!({
                "Source": "a@example.com",
                "Template": "t",
                "Destinations": [ { "Destination": {} } ]
            }),
            &ctx(),
        )
        .unwrap();
        let statuses = resp["Status"].as_array().unwrap();
        assert_eq!(statuses[0]["Status"], "MessageRejected");
    }

    #[test]
    fn send_bulk_templated_email_returns_not_found_for_missing_template() {
        let state = SesState::default();
        let err = send_bulk_templated_email(
            &state,
            &json!({
                "Source": "a@example.com",
                "Template": "missing",
                "Destinations": [
                    { "Destination": { "ToAddresses": ["b@example.com"] } }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "TemplateDoesNotExist");
    }

    #[test]
    fn send_email_rejects_paused_configuration_set() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "SendingOptions": { "SendingEnabled": false }
            }),
            &ctx(),
        )
        .unwrap();
        let err = send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "x" }, "Body": { "Text": { "Data": "x" } } } },
                "ConfigurationSetName": "cs"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccountSendingPausedException");
    }

    #[test]
    fn send_templated_email_rejects_paused_configuration_set() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "SendingOptions": { "SendingEnabled": false }
            }),
            &ctx(),
        )
        .unwrap();
        crate::operations::templates::create_email_template(
            &state,
            &json!({
                "TemplateName": "t",
                "TemplateContent": { "Subject": "s", "Text": "t" }
            }),
            &ctx(),
        )
        .unwrap();
        let err = send_templated_email(
            &state,
            &json!({
                "Source": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Template": "t",
                "TemplateData": "{}",
                "ConfigurationSetName": "cs"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccountSendingPausedException");
    }

    #[test]
    fn send_raw_email_rejects_paused_configuration_set() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "SendingOptions": { "SendingEnabled": false }
            }),
            &ctx(),
        )
        .unwrap();
        let err = send_raw_email(
            &state,
            &json!({
                "RawMessage": { "Data": "From: a@x.com\r\nTo: b@x.com\r\n\r\nhi" },
                "ConfigurationSetName": "cs"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AccountSendingPausedException");
    }

    #[test]
    fn send_email_allows_enabled_configuration_set() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::create_configuration_set(
            &state,
            &json!({
                "ConfigurationSetName": "cs",
                "SendingOptions": { "SendingEnabled": true }
            }),
            &ctx(),
        )
        .unwrap();
        send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "x" }, "Body": { "Text": { "Data": "x" } } } },
                "ConfigurationSetName": "cs"
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn send_raw_email_parses_rfc2822_headers_when_destinations_absent() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        crate::operations::more::create_configuration_set(
            &state,
            &json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .unwrap();
        let raw = "From: Alice <alice@example.com>\r\n\
                   To: Bob <bob@example.com>, charlie@example.com\r\n\
                   Cc: cc@example.com\r\n\
                   Reply-To: alice-reply@example.com\r\n\
                   Subject: Greetings\r\n\
                   \r\n\
                   Hello!";
        send_raw_email(
            &state,
            &json!({
                "RawMessage": { "Data": raw },
                "ConfigurationSetName": "cs"
            }),
            &ctx(),
        )
        .unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.from, "alice@example.com");
        assert_eq!(row.email.to, vec!["bob@example.com", "charlie@example.com"]);
        assert_eq!(row.email.cc, vec!["cc@example.com"]);
        assert_eq!(row.email.reply_to, vec!["alice-reply@example.com"]);
        assert_eq!(row.email.subject.as_deref(), Some("Greetings"));
        assert_eq!(row.email.configuration_set_name.as_deref(), Some("cs"));
        assert!(row.email.raw.as_deref().unwrap().contains("Hello!"));
    }

    #[test]
    fn send_raw_email_accepts_explicit_destinations() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        let raw = "Subject: Stand-in\r\n\r\nbody";
        send_raw_email(
            &state,
            &json!({
                "RawMessage": { "Data": raw },
                "Source": "from@example.com",
                "Destinations": ["explicit@example.com"],
                "Tags": [{ "Name": "k", "Value": "v" }]
            }),
            &ctx(),
        )
        .unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.from, "from@example.com");
        assert_eq!(row.email.to, vec!["explicit@example.com"]);
        assert_eq!(row.email.tags, vec![("k".to_string(), "v".to_string())]);
    }

    #[test]
    fn send_raw_email_decodes_base64_payload() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        let raw = "From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Encoded\r\n\r\nbody";
        let encoded = STANDARD.encode(raw.as_bytes());
        send_raw_email(
            &state,
            &json!({ "RawMessage": { "Data": encoded } }),
            &ctx(),
        )
        .unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.from, "alice@example.com");
        assert_eq!(row.email.subject.as_deref(), Some("Encoded"));
    }

    #[test]
    fn send_raw_email_folds_header_continuations() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        let raw = "From: alice@example.com\r\n\
                   To: a@example.com,\r\n\t b@example.com\r\n\
                   Subject: Fold\r\n\r\nbody";
        send_raw_email(&state, &json!({ "RawMessage": { "Data": raw } }), &ctx()).unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.to, vec!["a@example.com", "b@example.com"]);
    }

    #[test]
    fn send_email_records_cc_bcc_reply_to_recipients() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "Destination": {
                    "ToAddresses": ["primary@example.com"],
                    "CcAddresses": ["cc1@example.com", "cc2@example.com"],
                    "BccAddresses": ["bcc@example.com"]
                },
                "ReplyToAddresses": ["alice-reply@example.com"],
                "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } }
            }),
            &ctx(),
        )
        .unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.cc, vec!["cc1@example.com", "cc2@example.com"]);
        assert_eq!(row.email.bcc, vec!["bcc@example.com"]);
        assert_eq!(row.email.reply_to, vec!["alice-reply@example.com"]);
    }

    #[test]
    fn send_templated_email_renders_and_records_cc_bcc_reply_to() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
        crate::operations::templates::create_email_template(
            &state,
            &json!({
                "TemplateName": "welcome",
                "TemplateContent": {
                    "Subject": "Hi {{name}}",
                    "Text": "Welcome {{name}}.",
                }
            }),
            &ctx(),
        )
        .unwrap();
        send_templated_email(
            &state,
            &json!({
                "Source": "alice@example.com",
                "Destination": {
                    "ToAddresses": ["primary@example.com"],
                    "CcAddresses": ["cc@example.com"],
                    "BccAddresses": ["bcc@example.com"]
                },
                "ReplyToAddresses": ["alice-reply@example.com"],
                "Template": "welcome",
                "TemplateData": "{\"name\":\"Sam\"}"
            }),
            &ctx(),
        )
        .unwrap();
        let row = store.list_all().unwrap().into_iter().next().unwrap();
        assert_eq!(row.email.subject.as_deref(), Some("Hi Sam"));
        assert_eq!(row.email.body_text.as_deref(), Some("Welcome Sam."));
        assert_eq!(row.email.cc, vec!["cc@example.com"]);
        assert_eq!(row.email.bcc, vec!["bcc@example.com"]);
        assert_eq!(row.email.reply_to, vec!["alice-reply@example.com"]);
    }

    #[test]
    fn send_templated_email_rejects_missing_template() {
        let state = SesState::default();
        let err = send_templated_email(
            &state,
            &json!({
                "Source": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Template": "missing",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "TemplateDoesNotExist");
    }

    #[test]
    fn send_email_persists_configuration_set_and_tags() {
        let state = SesState::default();
        let path = std::env::temp_dir().join(format!("awsim-ses-tags-{}.db", uuid::Uuid::new_v4()));
        let store = std::sync::Arc::new(crate::SqliteStore::open(path).unwrap());
        state.set_sqlite(store.clone());
        crate::operations::more::create_configuration_set(
            &state,
            &json!({ "ConfigurationSetName": "cs" }),
            &ctx(),
        )
        .unwrap();
        let input = json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["recipient@example.com"] },
            "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } },
            "ConfigurationSetName": "cs",
            "EmailTags": [
                { "Name": "campaign", "Value": "spring" },
                { "Name": "env", "Value": "prod" }
            ],
        });
        send_email(&state, &input, &ctx()).unwrap();
        let rows = store.list_all().unwrap();
        let row = rows.first().expect("at least one email row");
        assert_eq!(row.email.configuration_set_name.as_deref(), Some("cs"));
        assert_eq!(row.email.tags.len(), 2);
        assert!(
            row.email
                .tags
                .iter()
                .any(|(k, v)| k == "campaign" && v == "spring")
        );
    }

    #[test]
    fn missing_configuration_set_is_rejected() {
        let state = SesState::default();
        let err = send_email(&state, &send_input("nope", false), &ctx()).unwrap_err();
        assert_eq!(err.code, "ConfigurationSetDoesNotExist");
    }

    #[test]
    fn send_email_accepts_routing_hint_params() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        let resp = send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "FromEmailAddressIdentityArn": "arn:aws:ses:us-east-1:111111111111:identity/example.com",
                "ReturnPath": "bounces@example.com",
                "ReturnPathArn": "arn:aws:ses:us-east-1:111111111111:identity/bounces.example.com",
                "FeedbackForwardingEmailAddress": "feedback@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["MessageId"].is_string());
    }

    #[test]
    fn send_email_rejects_malformed_from_identity_arn() {
        let state = SesState::default();
        let err = send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "FromEmailAddressIdentityArn": "not-an-arn",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("FromEmailAddressIdentityArn"));
    }

    #[test]
    fn send_email_rejects_account_suppressed_recipient() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::put_suppressed_destination(
            &state,
            &json!({ "EmailAddress": "bob@example.com", "Reason": "BOUNCE" }),
            &ctx(),
        )
        .unwrap();
        crate::operations::more::put_account_suppression_attributes(
            &state,
            &json!({ "SuppressedReasons": ["BOUNCE"] }),
            &ctx(),
        )
        .unwrap();
        let err = send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MessageRejected");
        assert!(err.message.contains("suppression"));
    }

    #[test]
    fn send_email_allows_when_no_suppressed_reasons_enforced() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store);
        crate::operations::more::put_suppressed_destination(
            &state,
            &json!({ "EmailAddress": "bob@example.com", "Reason": "BOUNCE" }),
            &ctx(),
        )
        .unwrap();
        crate::operations::more::put_account_suppression_attributes(
            &state,
            &json!({ "SuppressedReasons": [] }),
            &ctx(),
        )
        .unwrap();
        let resp = send_email(
            &state,
            &json!({
                "FromEmailAddress": "alice@example.com",
                "Destination": { "ToAddresses": ["bob@example.com"] },
                "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(resp["MessageId"].is_string());
    }
}

#[cfg(test)]
mod list_management_suppression_tests {
    use super::*;
    use crate::operations::more::{create_contact, create_contact_list};

    fn ctx() -> RequestContext {
        RequestContext::new("ses", "us-east-1")
    }

    fn seed_list(state: &SesState) {
        create_contact_list(
            state,
            &json!({
                "ContactListName": "marketing",
                "Topics": [{ "TopicName": "promos", "DisplayName": "Promos", "DefaultSubscriptionStatus": "OPT_IN" }],
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn base_send(extra: Value) -> Value {
        let mut input = json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["user@example.com"] },
            "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "hi" } } } },
        });
        let extra_map = extra.as_object().unwrap().clone();
        for (k, v) in extra_map {
            input[k] = v;
        }
        input
    }

    #[test]
    fn opt_out_topic_blocks_send() {
        let state = SesState::default();
        seed_list(&state);
        create_contact(
            &state,
            &json!({
                "ContactListName": "marketing",
                "EmailAddress": "user@example.com",
                "TopicPreferences": [
                    { "TopicName": "promos", "SubscriptionStatus": "OPT_OUT" }
                ],
            }),
            &ctx(),
        )
        .unwrap();
        let err = send_email(
            &state,
            &base_send(json!({
                "ListManagementOptions": { "ContactListName": "marketing", "TopicName": "promos" },
            })),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MessageRejected");
        assert!(err.message.contains("promos"));
    }

    #[test]
    fn opt_in_topic_allows_send() {
        let state = SesState::default();
        seed_list(&state);
        create_contact(
            &state,
            &json!({
                "ContactListName": "marketing",
                "EmailAddress": "user@example.com",
                "TopicPreferences": [
                    { "TopicName": "promos", "SubscriptionStatus": "OPT_IN" }
                ],
            }),
            &ctx(),
        )
        .unwrap();
        let resp = send_email(
            &state,
            &base_send(json!({
                "ListManagementOptions": { "ContactListName": "marketing", "TopicName": "promos" },
            })),
            &ctx(),
        )
        .unwrap();
        assert!(resp["MessageId"].is_string());
    }

    #[test]
    fn unsubscribe_all_blocks_any_topic() {
        let state = SesState::default();
        seed_list(&state);
        create_contact(
            &state,
            &json!({
                "ContactListName": "marketing",
                "EmailAddress": "user@example.com",
                "UnsubscribeAll": true,
            }),
            &ctx(),
        )
        .unwrap();
        let err = send_email(
            &state,
            &base_send(json!({
                "ListManagementOptions": { "ContactListName": "marketing" },
            })),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "MessageRejected");
        assert!(err.message.contains("unsubscribed"));
    }

    #[test]
    fn renders_templated_content() {
        let state = SesState::default();
        crate::operations::templates::create_email_template(
            &state,
            &json!({
                "TemplateName": "welcome",
                "TemplateContent": {
                    "Subject": "Hello {{name}}",
                    "Text": "Hi {{name}}, your code is {{code}}.",
                    "Html": "<p>Hi {{name}}</p>",
                },
            }),
            &RequestContext::new("ses", "us-east-1"),
        )
        .unwrap();

        let input = json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["user@example.com"] },
            "Content": {
                "Templated": {
                    "TemplateName": "welcome",
                    "TemplateData": "{\"name\":\"Alex\",\"code\":\"42\"}",
                },
            },
        });
        let resp = send_email(&state, &input, &RequestContext::new("ses", "us-east-1")).unwrap();
        assert!(resp["MessageId"].is_string());
    }

    #[test]
    fn templated_branch_rejects_missing_template() {
        let state = SesState::default();
        let input = json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["user@example.com"] },
            "Content": {
                "Templated": {
                    "TemplateName": "missing",
                    "TemplateData": "{}",
                },
            },
        });
        let err = send_email(&state, &input, &RequestContext::new("ses", "us-east-1")).unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }

    #[test]
    fn templated_branch_rejects_invalid_template_data() {
        let state = SesState::default();
        crate::operations::templates::create_email_template(
            &state,
            &json!({
                "TemplateName": "welcome",
                "TemplateContent": { "Subject": "x", "Text": "x" },
            }),
            &RequestContext::new("ses", "us-east-1"),
        )
        .unwrap();
        let input = json!({
            "FromEmailAddress": "sender@example.com",
            "Destination": { "ToAddresses": ["user@example.com"] },
            "Content": {
                "Templated": {
                    "TemplateName": "welcome",
                    "TemplateData": "{not-json",
                },
            },
        });
        let err = send_email(&state, &input, &RequestContext::new("ses", "us-east-1")).unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("TemplateData"));
    }

    #[test]
    fn render_template_substitutes_keys() {
        let data = json!({ "name": "Alex", "n": 7 });
        let out = render_template("Hi {{name}}, count={{n}}, missing={{x}}!", &data);
        assert_eq!(out, "Hi Alex, count=7, missing=!");
    }

    #[test]
    fn no_contact_record_defaults_to_allowed() {
        let state = SesState::default();
        seed_list(&state);
        let resp = send_email(
            &state,
            &base_send(json!({
                "ListManagementOptions": { "ContactListName": "marketing", "TopicName": "promos" },
            })),
            &ctx(),
        )
        .unwrap();
        assert!(resp["MessageId"].is_string());
    }
}

#[cfg(test)]
mod event_emission_tests {
    use super::*;
    use crate::state::{ConfigurationSet, EventDestination};

    fn cs_with_dest(state: &SesState, dest: EventDestination) {
        state.configuration_sets.insert(
            "cs".to_string(),
            ConfigurationSet {
                name: "cs".to_string(),
                sending_enabled: true,
                event_destinations: vec![dest],
                ..Default::default()
            },
        );
    }

    fn ctx_with_bus() -> (RequestContext, awsim_core::events::EventBus) {
        let bus = awsim_core::events::EventBus::new();
        let mut ctx = RequestContext::new("ses", "us-east-1");
        ctx.event_bus = Some(bus.clone());
        (ctx, bus)
    }

    fn send_input() -> Value {
        json!({
            "FromEmailAddress": "from@example.com",
            "Destination": { "ToAddresses": ["to@example.com"] },
            "ConfigurationSetName": "cs",
            "Content": { "Simple": { "Subject": { "Data": "hi" }, "Body": { "Text": { "Data": "yo" } } } },
        })
    }

    #[test]
    fn send_email_emits_event_to_matching_sns_destination() {
        let state = SesState::default();
        cs_with_dest(
            &state,
            EventDestination {
                name: "d1".into(),
                enabled: true,
                matching_event_types: vec!["SEND".into()],
                sns_topic_arn: Some("arn:aws:sns:us-east-1:000000000000:t".into()),
                firehose_delivery_stream_arn: None,
                cloudwatch_dimensions: vec![],
            },
        );
        let (ctx, bus) = ctx_with_bus();
        let mut rx = bus.subscribe();
        send_email(&state, &send_input(), &ctx).unwrap();
        let ev = rx.try_recv().expect("expected one ses:EmailEvent");
        assert_eq!(ev.event_type, "ses:EmailEvent");
        assert_eq!(ev.detail["destination"]["kind"], "sns");
        assert_eq!(ev.detail["eventType"], "SEND");
        assert_eq!(ev.detail["configurationSetName"], "cs");
        // Only SEND matched, so no second event.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn destination_not_matching_event_type_emits_nothing() {
        let state = SesState::default();
        cs_with_dest(
            &state,
            EventDestination {
                name: "d1".into(),
                enabled: true,
                matching_event_types: vec!["BOUNCE".into()],
                sns_topic_arn: Some("arn:aws:sns:us-east-1:000000000000:t".into()),
                firehose_delivery_stream_arn: None,
                cloudwatch_dimensions: vec![],
            },
        );
        let (ctx, bus) = ctx_with_bus();
        let mut rx = bus.subscribe();
        send_email(&state, &send_input(), &ctx).unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn disabled_destination_emits_nothing() {
        let state = SesState::default();
        cs_with_dest(
            &state,
            EventDestination {
                name: "d1".into(),
                enabled: false,
                matching_event_types: vec!["SEND".into(), "DELIVERY".into()],
                sns_topic_arn: Some("arn:aws:sns:us-east-1:000000000000:t".into()),
                firehose_delivery_stream_arn: None,
                cloudwatch_dimensions: vec![],
            },
        );
        let (ctx, bus) = ctx_with_bus();
        let mut rx = bus.subscribe();
        send_email(&state, &send_input(), &ctx).unwrap();
        assert!(rx.try_recv().is_err());
    }
}
