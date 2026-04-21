use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{EmailTemplate, SesState};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// CreateEmailTemplate
// ---------------------------------------------------------------------------

pub fn create_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;

    let content = &input["TemplateContent"];
    let subject = content["Subject"].as_str().map(String::from);
    let html = content["Html"].as_str().map(String::from);
    let text = content["Text"].as_str().map(String::from);

    let template = EmailTemplate {
        name: name.to_string(),
        subject,
        html,
        text,
        created_at: now_epoch(),
    };

    info!(template = %name, "SES: created email template");
    state.templates.insert(name.to_string(), template);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DeleteEmailTemplate
// ---------------------------------------------------------------------------

pub fn delete_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;

    if state.templates.remove(name).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Template not found: {name}"),
        ));
    }

    info!(template = %name, "SES: deleted email template");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetEmailTemplate
// ---------------------------------------------------------------------------

pub fn get_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;

    let t = state.templates.get(name).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("Template not found: {name}"))
    })?;

    Ok(json!({
        "TemplateName": t.name,
        "TemplateContent": {
            "Subject": t.subject,
            "Html": t.html,
            "Text": t.text
        }
    }))
}

// ---------------------------------------------------------------------------
// ListEmailTemplates
// ---------------------------------------------------------------------------

pub fn list_email_templates(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let templates: Vec<Value> = state
        .templates
        .iter()
        .map(|e| {
            json!({
                "TemplateName": e.name,
                "CreatedTimestamp": e.created_at
            })
        })
        .collect();

    Ok(json!({ "TemplatesMetadata": templates }))
}
