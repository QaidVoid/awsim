//! Outbound Cognito email delivery.
//!
//! Real Cognito sends verification codes, password-reset codes, and admin
//! invitations to the user's email (via its own mailer or, when configured,
//! your SES account). awsim has no real mailer, so instead of dropping these
//! messages it publishes them on the internal event bus; the gateway routes
//! `cognito:EmailDelivery` events to the SES service, where they land in the
//! same sent-email store the SES console/UI reads. That gives one place to
//! inspect every code an emulated app would have received.
//!
//! Subjects and bodies honour the pool's configured message templates
//! (`VerificationMessageTemplate`, `EmailVerificationMessage/Subject`,
//! `AdminCreateUserConfig.InviteMessageTemplate`) when set, substituting the
//! Cognito placeholders `{####}` (the code / temporary password) and
//! `{username}`; otherwise sensible defaults are used.

use std::collections::HashMap;

use awsim_core::{InternalEvent, RequestContext};
use serde_json::{Value, json};

/// The from-address Cognito uses for COGNITO_DEFAULT email sending.
const DEFAULT_FROM: &str = "no-reply@verificationemail.com";

/// Event type the gateway routes to the SES delivery integration.
pub const EVENT_TYPE: &str = "cognito:EmailDelivery";

/// Publish an outbound email onto the event bus for SES delivery. A no-op when
/// there is no recipient or no bus wired (e.g. unit tests of the operation in
/// isolation).
pub fn deliver(ctx: &RequestContext, to: &str, subject: &str, body: &str, message_type: &str) {
    if to.is_empty() {
        return;
    }
    if let Some(bus) = &ctx.event_bus {
        bus.publish(InternalEvent {
            source: "cognito-idp".to_string(),
            event_type: EVENT_TYPE.to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "from": DEFAULT_FROM,
                "to": to,
                "subject": subject,
                "body": body,
                "messageType": message_type,
            }),
        });
    }
}

/// The pool's extra-config map (where verification / invite templates live).
type Config = HashMap<String, Value>;

/// Read a string out of the pool's extra config, descending one nested key
/// when `nested` is given (e.g. `VerificationMessageTemplate.EmailSubject`).
fn cfg<'a>(config: &'a Config, key: &str, nested: Option<(&str, &str)>) -> Option<&'a str> {
    if let Some(direct) = config.get(key).and_then(|v| v.as_str()) {
        return Some(direct);
    }
    if let Some((outer, inner)) = nested {
        return config.get(outer)?.get(inner)?.as_str();
    }
    None
}

/// Subject and body for an email-verification / confirmation code.
pub fn verification_message(config: &Config, code: &str) -> (String, String) {
    let subject = cfg(
        config,
        "EmailVerificationSubject",
        Some(("VerificationMessageTemplate", "EmailSubject")),
    )
    .unwrap_or("Your verification code");
    let message = cfg(
        config,
        "EmailVerificationMessage",
        Some(("VerificationMessageTemplate", "EmailMessage")),
    )
    .unwrap_or("Your verification code is {####}");
    (subject.to_string(), message.replace("{####}", code))
}

/// Subject and body for a forgot-password reset code. Cognito reuses the
/// verification template for account recovery.
pub fn forgot_password_message(config: &Config, code: &str) -> (String, String) {
    // Keep the configured template if the pool set one; otherwise a
    // reset-specific default reads better than the generic one.
    if cfg(
        config,
        "EmailVerificationMessage",
        Some(("VerificationMessageTemplate", "EmailMessage")),
    )
    .is_some()
    {
        verification_message(config, code)
    } else {
        (
            "Your password reset code".to_string(),
            format!("Your password reset code is {code}"),
        )
    }
}

/// Subject and body for an admin-created user's invitation. `{####}` is the
/// temporary password and `{username}` the sign-in name.
pub fn invitation_message(
    config: &Config,
    username: &str,
    temp_password: &str,
) -> (String, String) {
    let invite = config
        .get("AdminCreateUserConfig")
        .and_then(|c| c.get("InviteMessageTemplate"));
    let subject = invite
        .and_then(|t| t.get("EmailSubject"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your temporary password");
    let message = invite
        .and_then(|t| t.get("EmailMessage"))
        .and_then(|v| v.as_str())
        .unwrap_or("Your username is {username} and temporary password is {####}.");
    (
        subject.to_string(),
        message
            .replace("{username}", username)
            .replace("{####}", temp_password),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verification_uses_default_when_unconfigured() {
        let (subject, body) = verification_message(&Config::new(), "123456");
        assert_eq!(subject, "Your verification code");
        assert_eq!(body, "Your verification code is 123456");
    }

    #[test]
    fn verification_honours_configured_template() {
        let mut cfg = Config::new();
        cfg.insert("EmailVerificationSubject".to_string(), json!("Verify!"));
        cfg.insert(
            "EmailVerificationMessage".to_string(),
            json!("Code: {####} - thanks"),
        );
        let (subject, body) = verification_message(&cfg, "987654");
        assert_eq!(subject, "Verify!");
        assert_eq!(body, "Code: 987654 - thanks");
    }

    #[test]
    fn invitation_substitutes_username_and_password() {
        let mut cfg = Config::new();
        cfg.insert(
            "AdminCreateUserConfig".to_string(),
            json!({ "InviteMessageTemplate": {
                "EmailSubject": "Welcome", "EmailMessage": "Hi {username}, pw {####}"
            }}),
        );
        let (subject, body) = invitation_message(&cfg, "bob", "Temp@1");
        assert_eq!(subject, "Welcome");
        assert_eq!(body, "Hi bob, pw Temp@1");
    }
}
