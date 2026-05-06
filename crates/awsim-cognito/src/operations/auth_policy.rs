//! Cognito password-policy validation and account-lockout bookkeeping.
//!
//! Real Cognito enforces password policies at every password-mutating
//! operation (SignUp, AdminCreateUser, AdminSetUserPassword, ChangePassword,
//! ConfirmForgotPassword) and surfaces failures via `InvalidPasswordException`
//! with a message describing which constraint was violated.
//!
//! Account lockout in real AWS Cognito is part of Advanced Security (paid),
//! but a lightweight version is invaluable for local testing — without it,
//! brute-force or runaway-loop bugs in client code never surface during
//! development. We model a fixed 5-strikes / 15-minute window so that
//! integration tests against a misconfigured client see the same
//! `NotAuthorizedException` they would in production.

use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::AwsError;
use serde_json::Value;
use uuid::Uuid;

use crate::state::{AuthEvent, CognitoUser, PasswordPolicy, UserPool};

/// Number of consecutive failed authentication attempts that triggers a
/// lockout. Matches real Cognito Advanced Security defaults.
pub const MAX_FAILED_LOGINS: u32 = 5;

/// How long the account stays locked after hitting `MAX_FAILED_LOGINS`.
pub const LOCKOUT_DURATION_SECS: u64 = 15 * 60;

/// Cap on per-user `auth_events` so a long-lived emulator doesn't accumulate
/// unbounded history. AdminListUserAuthEvents returns the most recent first.
pub const MAX_AUTH_EVENTS_PER_USER: usize = 100;

/// Built-in compromised-password list — lets tests exercise the BLOCK action
/// without needing to wire an external feed. Real Cognito Advanced Security
/// uses a much larger AWS-curated dataset.
const COMPROMISED_PASSWORDS: &[&str] = &[
    "password",
    "Password1",
    "Password1!",
    "12345678",
    "qwerty",
    "letmein",
    "iloveyou",
    "admin",
    "welcome",
    "abc123",
    "monkey",
    "football",
];

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Verify that `password` satisfies every constraint in `policy`. Returns a
/// Cognito-shaped `InvalidPasswordException` with a message naming the first
/// violated rule.
pub fn validate_password(policy: &PasswordPolicy, password: &str) -> Result<(), AwsError> {
    if (password.chars().count() as u32) < policy.minimum_length {
        return Err(invalid_password(format!(
            "Password did not conform with policy: Password must have at least {} character(s)",
            policy.minimum_length
        )));
    }
    if policy.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
        return Err(invalid_password(
            "Password did not conform with policy: Password must have a lowercase character",
        ));
    }
    if policy.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
        return Err(invalid_password(
            "Password did not conform with policy: Password must have an uppercase character",
        ));
    }
    if policy.require_numbers && !password.chars().any(|c| c.is_ascii_digit()) {
        return Err(invalid_password(
            "Password did not conform with policy: Password must have a numeric character",
        ));
    }
    if policy.require_symbols && !password.chars().any(is_symbol) {
        return Err(invalid_password(
            "Password did not conform with policy: Password must have a symbol character",
        ));
    }
    Ok(())
}

/// Reject the call when the user is currently inside a lockout window.
/// Clears the expired lockout silently so the user is auth-eligible again.
pub fn check_not_locked(user: &mut CognitoUser) -> Result<(), AwsError> {
    let now = now_epoch();
    if let Some(until) = user.locked_until_secs {
        if until > now {
            return Err(AwsError::bad_request(
                "NotAuthorizedException",
                "Password attempts exceeded",
            ));
        }
        // Window has elapsed — reset so the user can try again.
        user.locked_until_secs = None;
        user.failed_login_attempts = 0;
    }
    Ok(())
}

/// Record the result of an authentication attempt. On failure increments the
/// counter and locks the account at the threshold; on success resets both
/// fields.
pub fn record_attempt(user: &mut CognitoUser, success: bool) {
    if success {
        user.failed_login_attempts = 0;
        user.locked_until_secs = None;
        return;
    }
    user.failed_login_attempts = user.failed_login_attempts.saturating_add(1);
    if user.failed_login_attempts >= MAX_FAILED_LOGINS {
        user.locked_until_secs = Some(now_epoch() + LOCKOUT_DURATION_SECS);
    }
}

fn invalid_password(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("InvalidPasswordException", message)
}

/// Decide whether the supplied password is on the built-in compromised list.
pub fn is_compromised_password(password: &str) -> bool {
    COMPROMISED_PASSWORDS.contains(&password)
}

/// Look up the active `CompromisedCredentialsRiskConfiguration` for the given
/// client (falling back to the pool-level default) and return the configured
/// EventAction (`BLOCK` or `NO_ACTION`) for the SIGN_IN event filter.
pub fn compromised_credentials_action_for(
    pool: &UserPool,
    client_id: Option<&str>,
    event_type: &str,
) -> Option<String> {
    let key = client_id.unwrap_or("pool");
    let config = pool
        .risk_configurations
        .iter()
        .find(|c| c.client_id.as_deref().unwrap_or("pool") == key)
        .or_else(|| {
            pool.risk_configurations
                .iter()
                .find(|c| c.client_id.is_none())
        })?;
    let cfg = config.compromised_credentials_config.as_ref()?;
    let filters = cfg
        .get("EventFilter")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !filters.is_empty() && !filters.iter().any(|f| f == event_type) {
        return None;
    }
    cfg.get("Actions")
        .and_then(|a| a.get("EventAction"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Append a new auth event onto the user's bounded history, dropping the
/// oldest entry once the cap is reached.
pub fn record_auth_event(user: &mut CognitoUser, event: AuthEvent) {
    user.auth_events.push(event);
    if user.auth_events.len() > MAX_AUTH_EVENTS_PER_USER {
        let drop = user.auth_events.len() - MAX_AUTH_EVENTS_PER_USER;
        user.auth_events.drain(0..drop);
    }
}

/// Build a fresh sign-in event with the given outcome + risk classification.
pub fn build_signin_event(success: bool, compromised: bool) -> AuthEvent {
    let risk_decision = if compromised {
        "AccountTakeover".to_string()
    } else {
        "NoRisk".to_string()
    };
    let risk_level = if compromised || !success {
        "Medium".to_string()
    } else {
        "Low".to_string()
    };
    AuthEvent {
        event_id: Uuid::new_v4().to_string(),
        event_type: "SignIn".to_string(),
        creation_date: now_epoch(),
        event_response: if success {
            "Pass".to_string()
        } else {
            "Fail".to_string()
        },
        risk_level,
        risk_decision,
        compromised_credentials_detected: compromised,
        feedback_value: None,
    }
}

fn is_symbol(c: char) -> bool {
    // Cognito's symbol set per the docs.
    "^$*.[]{}()?\"!@#%&/\\,><':;|_~`=+- ".contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> CognitoUser {
        CognitoUser {
            username: "u".into(),
            sub: "s".into(),
            password: "Hunter2!".into(),
            attributes: Default::default(),
            status: "CONFIRMED".into(),
            enabled: true,
            groups: Vec::new(),
            created_date: 0,
            pending_verifications: Default::default(),
            revoked_refresh_tokens: Vec::new(),
            mfa_enabled: false,
            mfa_preferred: None,
            totp_secret: None,
            totp_verified: false,
            devices: Vec::new(),
            linked_providers: Vec::new(),
            mfa_options: Vec::new(),
            webauthn_credentials: Vec::new(),
            webauthn_pending_challenge: None,
            failed_login_attempts: 0,
            locked_until_secs: None,
            auth_events: Vec::new(),
        }
    }

    #[test]
    fn validate_password_enforces_each_rule() {
        let policy = PasswordPolicy {
            minimum_length: 8,
            require_lowercase: true,
            require_uppercase: true,
            require_numbers: true,
            require_symbols: true,
            temporary_password_validity_days: 7,
        };

        // Happy path.
        validate_password(&policy, "Hunter2!").unwrap();

        // Each individual rule fires.
        let cases = [
            ("Short1!", "at least"),
            ("ALLCAPS1!", "lowercase"),
            ("alllower1!", "uppercase"),
            ("NoDigits!", "numeric"),
            ("NoSymbol1", "symbol"),
        ];
        for (pw, contains) in cases {
            let err = validate_password(&policy, pw).unwrap_err();
            assert_eq!(err.code, "InvalidPasswordException");
            assert!(
                err.message.contains(contains),
                "expected message to mention {contains:?}: {}",
                err.message
            );
        }
    }

    #[test]
    fn compromised_credentials_action_reads_from_pool_config() {
        use crate::state::{RiskConfiguration, UserPool};

        let mut pool = UserPool {
            id: "p1".into(),
            name: "p".into(),
            arn: "arn".into(),
            clients: Default::default(),
            users: Default::default(),
            groups: Default::default(),
            created_date: 0,
            policies: PasswordPolicy::default(),
            mfa_configuration: "OFF".into(),
            software_token_mfa_enabled: false,
            auto_verified_attributes: Vec::new(),
            username_attributes: Vec::new(),
            alias_attributes: Vec::new(),
            lambda_config: Default::default(),
            schema: Vec::new(),
            email_configuration: None,
            domain: None,
            resource_servers: Vec::new(),
            identity_providers: Vec::new(),
            tags: Default::default(),
            ui_customizations: Default::default(),
            managed_login_brandings: Vec::new(),
            risk_configurations: Vec::new(),
            import_jobs: Vec::new(),
            log_delivery_configuration: None,
            terms: Vec::new(),
        };

        // No risk config — None.
        assert!(compromised_credentials_action_for(&pool, None, "SIGN_IN").is_none());

        // Pool-level config that blocks SIGN_IN.
        pool.risk_configurations.push(RiskConfiguration {
            client_id: None,
            compromised_credentials_config: Some(serde_json::json!({
                "EventFilter": ["SIGN_IN"],
                "Actions": { "EventAction": "BLOCK" }
            })),
            account_takeover_config: None,
            risk_exception_config: None,
        });
        assert_eq!(
            compromised_credentials_action_for(&pool, None, "SIGN_IN").as_deref(),
            Some("BLOCK")
        );
        // Filter excludes other event types.
        assert!(compromised_credentials_action_for(&pool, None, "PASSWORD_CHANGE").is_none());
    }

    #[test]
    fn known_weak_passwords_classified_as_compromised() {
        assert!(is_compromised_password("password"));
        assert!(is_compromised_password("12345678"));
        assert!(!is_compromised_password("Hunter2!secure"));
    }

    #[test]
    fn lockout_engages_at_threshold_and_releases_on_success() {
        let mut u = user();

        for _ in 0..MAX_FAILED_LOGINS - 1 {
            record_attempt(&mut u, false);
            check_not_locked(&mut u).unwrap();
        }
        record_attempt(&mut u, false);
        // The Nth failure locks the account.
        let err = check_not_locked(&mut u).unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
        assert!(u.locked_until_secs.is_some());

        // Simulating wall-clock progress past the lockout window.
        u.locked_until_secs = Some(now_epoch().saturating_sub(1));
        check_not_locked(&mut u).unwrap();
        assert!(u.locked_until_secs.is_none());
        assert_eq!(u.failed_login_attempts, 0);

        // A successful attempt resets the counter wholesale.
        record_attempt(&mut u, false);
        record_attempt(&mut u, true);
        assert_eq!(u.failed_login_attempts, 0);
        assert!(u.locked_until_secs.is_none());
    }
}
