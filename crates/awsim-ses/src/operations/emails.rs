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
    Ok(json!({ "MessageId": message_id }))
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

    // Configuration set lookup + TLS policy enforcement. When the set is
    // configured with TlsPolicy=REQUIRE we refuse sends that the caller
    // tags as plaintext-only via the `aws-ses-disable-tls=true` email
    // tag. This mirrors the AWS behavior of refusing delivery when the
    // recipient MTA cannot negotiate TLS.
    if let Some(ref cs_name) = configuration_set_name {
        let cs = state.configuration_sets.get(cs_name).ok_or_else(|| {
            AwsError::not_found(
                "ConfigurationSetDoesNotExist",
                format!("Configuration set does not exist: {cs_name}"),
            )
        })?;
        if cs.tls_policy.as_deref() == Some("REQUIRE") && tags_signal_no_tls(input.get("EmailTags"))
        {
            return Err(AwsError::bad_request(
                "MessageRejected",
                "ConfigurationSet TlsPolicy is REQUIRE; recipient does not support TLS",
            ));
        }
    }

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
    fn send_raw_email_parses_rfc2822_headers_when_destinations_absent() {
        let state = SesState::default();
        let store = open_store();
        state.set_sqlite(store.clone());
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
