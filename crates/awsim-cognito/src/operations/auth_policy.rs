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

use crate::state::{CognitoUser, PasswordPolicy};

/// Number of consecutive failed authentication attempts that triggers a
/// lockout. Matches real Cognito Advanced Security defaults.
pub const MAX_FAILED_LOGINS: u32 = 5;

/// How long the account stays locked after hitting `MAX_FAILED_LOGINS`.
pub const LOCKOUT_DURATION_SECS: u64 = 15 * 60;

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
