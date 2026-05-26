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
