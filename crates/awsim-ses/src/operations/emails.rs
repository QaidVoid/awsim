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

    // Content — Simple or Raw
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
    } else {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Content must include Simple or Raw",
        ));
    };

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
        raw,
        sent_at: now_epoch(),
        configuration_set_name,
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
