use std::collections::HashMap;

use awsim_core::{AwsError, InternalEvent, LambdaInvoker, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::jwt::{self, GroupRolePair};
use crate::state::{
    CognitoState, CognitoUser, CustomAuthRound, CustomAuthSession, MfaSession, UserPool,
    UserPoolClient,
};

struct TokenValidity {
    access: u64,
    id: u64,
}

impl TokenValidity {
    fn from_client(client: &UserPoolClient) -> Self {
        Self {
            access: client.access_token_validity,
            id: client.id_token_validity,
        }
    }

    fn defaults() -> Self {
        Self {
            access: 3600,
            id: 3600,
        }
    }
}

/// MFA / SRP challenge sessions live this long before the server forgets
/// them, matching Cognito's 5-minute default `auth_session_validity`.
const SESSION_VALIDITY_SECS: u64 = 5 * 60;

fn now_epoch() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn session_still_valid(issued_at: u64) -> bool {
    now_epoch().saturating_sub(issued_at) < SESSION_VALIDITY_SECS
}

/// `pending_verifications` key under which a freshly issued SMS_MFA code is
/// stashed for `RespondToAuthChallenge(SMS_MFA)`.
const SMS_MFA_KEY: &str = "SMS_MFA";
/// `pending_verifications` key for an issued EMAIL_OTP code.
const EMAIL_OTP_KEY: &str = "EMAIL_OTP";

/// Generate a 6-digit numeric MFA / OTP code.
fn generate_mfa_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000u32))
}

/// The MFA challenge to issue after a user's password is verified, when MFA is
/// required. Branches on the user's preferred factor and the factors actually
/// configured. When SMS or email is chosen, a code is generated and stashed on
/// the user (under `pending_verifications`) so the response arm can verify it.
enum MfaChallenge {
    /// SMS_MFA: a code was stashed; carry the masked destination back.
    Sms { destination: String },
    /// EMAIL_OTP: a code was stashed; carry the masked destination back.
    EmailOtp { destination: String },
    /// SELECT_MFA_TYPE: multiple factors are enabled with no clear preference.
    Select,
    /// SOFTWARE_TOKEN_MFA (the default).
    SoftwareToken,
}

/// Whether the user can complete a software-token (TOTP) challenge.
fn software_factor_enabled(user: &CognitoUser) -> bool {
    user.totp_verified
}

/// Whether the user can receive an SMS_MFA code. awsim has no dedicated
/// per-factor enabled flag beyond the preference, so SMS counts as available
/// when the user prefers it or has a phone number on file with MFA enabled.
fn sms_factor_enabled(user: &CognitoUser) -> bool {
    user.mfa_preferred.as_deref() == Some("SMS_MFA")
        || (user.mfa_enabled && user.attributes.contains_key("phone_number"))
}

/// Whether the user can receive an EMAIL_OTP code. Driven purely by the
/// preference, since awsim has no separate email-MFA enabled flag.
fn email_factor_enabled(user: &CognitoUser) -> bool {
    user.mfa_preferred.as_deref() == Some("EMAIL_OTP")
}

/// Mask an SMS / email destination the way Cognito does in
/// `CodeDeliveryDetails`, e.g. `+12345550100` -> `+*******0100`.
fn mask_destination(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    let keep = 4;
    let masked: String = chars[..chars.len() - keep].iter().map(|_| '*').collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{masked}{tail}")
}

/// Decide which MFA challenge to issue and, for code-based factors, stash a
/// fresh code on the user. Returns `None` when no MFA factor is configured.
fn issue_mfa_challenge(user: &mut CognitoUser) -> Option<MfaChallenge> {
    let software = software_factor_enabled(user);
    let sms = sms_factor_enabled(user);

    match user.mfa_preferred.as_deref() {
        Some("SMS_MFA") if sms => {
            let code = generate_mfa_code();
            let destination = user
                .attributes
                .get("phone_number")
                .map(|p| mask_destination(p))
                .unwrap_or_else(|| "+*******".to_string());
            user.pending_verifications
                .insert(SMS_MFA_KEY.to_string(), code);
            user.pending_verifications_issued
                .insert(SMS_MFA_KEY.to_string(), now_epoch());
            Some(MfaChallenge::Sms { destination })
        }
        Some("EMAIL_OTP") => {
            let code = generate_mfa_code();
            let destination = user
                .attributes
                .get("email")
                .map(|e| mask_destination(e))
                .unwrap_or_else(|| "*@*".to_string());
            user.pending_verifications
                .insert(EMAIL_OTP_KEY.to_string(), code);
            user.pending_verifications_issued
                .insert(EMAIL_OTP_KEY.to_string(), now_epoch());
            Some(MfaChallenge::EmailOtp { destination })
        }
        Some("SOFTWARE_TOKEN_MFA") if software => Some(MfaChallenge::SoftwareToken),
        // No explicit (or no usable) preference: SELECT_MFA_TYPE when more than
        // one factor is on, otherwise fall back to whichever single factor is
        // configured, defaulting to software-token.
        _ => {
            if software && sms {
                Some(MfaChallenge::Select)
            } else if sms {
                let code = generate_mfa_code();
                let destination = user
                    .attributes
                    .get("phone_number")
                    .map(|p| mask_destination(p))
                    .unwrap_or_else(|| "+*******".to_string());
                user.pending_verifications
                    .insert(SMS_MFA_KEY.to_string(), code);
                user.pending_verifications_issued
                    .insert(SMS_MFA_KEY.to_string(), now_epoch());
                Some(MfaChallenge::Sms { destination })
            } else if software {
                Some(MfaChallenge::SoftwareToken)
            } else {
                None
            }
        }
    }
}

/// Build the JSON challenge response for an [`MfaChallenge`], inserting the
/// session into the MFA session store.
fn mfa_challenge_response(
    state: &CognitoState,
    pool_id: &str,
    username: &str,
    challenge: MfaChallenge,
) -> Value {
    let session_id = Uuid::new_v4().to_string();
    state.mfa_sessions.insert(
        session_id.clone(),
        MfaSession {
            pool_id: pool_id.to_string(),
            username: username.to_string(),
            issued_at: now_epoch(),
        },
    );

    match challenge {
        MfaChallenge::Sms { destination } => json!({
            "ChallengeName": "SMS_MFA",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "CODE_DELIVERY_DELIVERY_MEDIUM": "SMS",
                "CODE_DELIVERY_DESTINATION": destination,
            },
            "CodeDeliveryDetails": {
                "DeliveryMedium": "SMS",
                "Destination": destination,
                "AttributeName": "phone_number",
            }
        }),
        MfaChallenge::EmailOtp { destination } => json!({
            "ChallengeName": "EMAIL_OTP",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "CODE_DELIVERY_DELIVERY_MEDIUM": "EMAIL",
                "CODE_DELIVERY_DESTINATION": destination,
            },
            "CodeDeliveryDetails": {
                "DeliveryMedium": "EMAIL",
                "Destination": destination,
                "AttributeName": "email",
            }
        }),
        MfaChallenge::Select => json!({
            "ChallengeName": "SELECT_MFA_TYPE",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "MFAS_CAN_CHOOSE": "[\"SMS_MFA\",\"SOFTWARE_TOKEN_MFA\"]",
            }
        }),
        MfaChallenge::SoftwareToken => json!({
            "ChallengeName": "SOFTWARE_TOKEN_MFA",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "FRIENDLY_DEVICE_NAME": "TOTP device",
            }
        }),
    }
}

/// Build an MFA_SETUP challenge for a user who must configure MFA before
/// sign-in can complete (the pool's MfaConfiguration is ON but the user has no
/// usable factor yet). Registers a session so the follow-up
/// AssociateSoftwareToken / SetUserMFAPreference flow resolves back to the user.
fn mfa_setup_challenge(state: &CognitoState, pool_id: &str, username: &str) -> Value {
    let session_id = Uuid::new_v4().to_string();
    state.mfa_sessions.insert(
        session_id.clone(),
        MfaSession {
            pool_id: pool_id.to_string(),
            username: username.to_string(),
            issued_at: now_epoch(),
        },
    );
    json!({
        "ChallengeName": "MFA_SETUP",
        "Session": session_id,
        "ChallengeParameters": {
            "USER_ID_FOR_SRP": username,
            "MFAS_CAN_SETUP": "[\"SOFTWARE_TOKEN_MFA\"]",
        }
    })
}

/// Register a challenge session binding a session id to the user who just
/// passed primary authentication, and return that id. Used for the
/// NEW_PASSWORD_REQUIRED challenge so the follow-up response can prove the
/// caller actually completed the password step (without it, anyone knowing a
/// ClientId and username could reset any user's password).
fn issue_challenge_session(state: &CognitoState, pool_id: &str, username: &str) -> String {
    let session_id = Uuid::new_v4().to_string();
    state.mfa_sessions.insert(
        session_id.clone(),
        MfaSession {
            pool_id: pool_id.to_string(),
            username: username.to_string(),
            issued_at: now_epoch(),
        },
    );
    session_id
}

/// Resolve and validate a challenge session, returning the bound username.
/// The session must exist, be unexpired, belong to `pool_id`, and (when the
/// caller supplied a USERNAME in ChallengeResponses) match it. An expired
/// session is consumed. Mirrors Cognito's "Invalid session for the user."
/// rejection of forged or stale sessions.
fn resolve_challenge_session(
    state: &CognitoState,
    pool_id: &str,
    session_id: &str,
    claimed_username: Option<&str>,
) -> Result<String, AwsError> {
    let session = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }
    if session.pool_id != pool_id {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user.",
        ));
    }
    if let Some(claimed) = claimed_username
        && claimed != session.username
    {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user.",
        ));
    }
    Ok(session.username)
}

/// Serialize a user's attributes as the JSON-object string Cognito puts in the
/// `userAttributes` ChallengeParameter (e.g. `{"email":"x"}`), which is what
/// Amplify parses to prefill the NEW_PASSWORD_REQUIRED form. (The legacy array
/// of `{"Name","Value"}` pairs is not what the service sends.)
fn user_attributes_param(user: &CognitoUser) -> String {
    let map: serde_json::Map<String, Value> = user
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// Build the concrete-factor challenge JSON after `SELECT_MFA_TYPE`, reusing
/// the caller's existing session id so the follow-up response resolves back to
/// the same user.
fn mfa_select_followup(session_id: &str, username: &str, challenge: MfaChallenge) -> Value {
    match challenge {
        MfaChallenge::Sms { destination } => json!({
            "ChallengeName": "SMS_MFA",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "CODE_DELIVERY_DELIVERY_MEDIUM": "SMS",
                "CODE_DELIVERY_DESTINATION": destination,
            },
            "CodeDeliveryDetails": {
                "DeliveryMedium": "SMS",
                "Destination": destination,
                "AttributeName": "phone_number",
            }
        }),
        _ => json!({
            "ChallengeName": "SOFTWARE_TOKEN_MFA",
            "Session": session_id,
            "ChallengeParameters": {
                "USER_ID_FOR_SRP": username,
                "FRIENDLY_DEVICE_NAME": "TOTP device",
            }
        }),
    }
}

/// Verify a code-based MFA challenge (SMS_MFA / EMAIL_OTP) against the value
/// stashed on the user, mint tokens on success, and consume the session.
///
/// `code_field` is the `ChallengeResponses` key carrying the submitted code
/// (`SMS_MFA_CODE` / `EMAIL_OTP_CODE`) and `stash_key` is the
/// `pending_verifications` key the issuance side wrote under.
fn verify_code_mfa(
    state: &CognitoState,
    client_id: &str,
    region: &str,
    input: &Value,
    code_field: &str,
    stash_key: &str,
) -> Result<Value, AwsError> {
    let session_id = input["Session"].as_str().unwrap_or("");
    let user_code = input["ChallengeResponses"][code_field]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                format!("ChallengeResponses.{code_field} is required"),
            )
        })?;

    let session_meta = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session_meta.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }

    let pool = state.user_pools.get(&session_meta.pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "Pool not found")
    })?;
    let user = pool.users.get(&session_meta.username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let expected = user
        .pending_verifications
        .get(stash_key)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "No MFA code was issued"))?;
    if expected.as_str() != user_code {
        return Err(AwsError::bad_request(
            "CodeMismatchException",
            "Invalid code received for user",
        ));
    }

    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    let result = build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        &session_meta.pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    );
    drop(pool);

    // Clear the consumed code and session.
    if let Some(mut pool) = state.user_pools.get_mut(&session_meta.pool_id)
        && let Some(user) = pool.users.get_mut(&session_meta.username)
    {
        user.pending_verifications.remove(stash_key);
        user.pending_verifications_issued.remove(stash_key);
    }
    state.mfa_sessions.remove(session_id);
    info!(username = %session_meta.username, challenge = %stash_key, "Cognito: code-based MFA success");
    Ok(result)
}

/// Verify a SOFTWARE_TOKEN_MFA (TOTP) challenge response and, on success, mint
/// tokens. Shared by `RespondToAuthChallenge` and `AdminRespondToAuthChallenge`
/// so the admin and non-admin flows cannot drift apart on verification.
///
/// The session is looked up without being consumed so that a wrong code
/// surfaces `CodeMismatchException` and can be retried; the session is dropped
/// only once the code verifies.
fn complete_software_token_mfa(
    state: &CognitoState,
    client_id: &str,
    region: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    let session_id = input["Session"].as_str().unwrap_or("");
    let user_code = input["ChallengeResponses"]["SOFTWARE_TOKEN_MFA_CODE"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "ChallengeResponses.SOFTWARE_TOKEN_MFA_CODE is required",
            )
        })?;

    let session_meta = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session_meta.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }

    let pool = state.user_pools.get(&session_meta.pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "Pool not found")
    })?;
    let user = pool.users.get(&session_meta.username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let secret = user.totp_secret.as_deref().ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            "User has no software token configured",
        )
    })?;
    if !awsim_core::totp::verify_str(secret, user_code, 1) {
        return Err(AwsError::bad_request(
            "CodeMismatchException",
            "Invalid code received for user",
        ));
    }

    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    let result = build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        &session_meta.pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    );
    drop(pool);
    state.mfa_sessions.remove(session_id);
    info!(username = %session_meta.username, "Cognito: SOFTWARE_TOKEN_MFA success");
    Ok(result)
}

/// Complete a `RespondToAuthChallenge(MFA_SETUP)`. By this point the client
/// has, within the same session, run AssociateSoftwareToken +
/// VerifySoftwareToken (which set the user's `totp_verified`). If the software
/// token is now verified we record the preference, mint tokens, and consume the
/// session; otherwise the setup is incomplete and we re-issue the MFA_SETUP
/// challenge so the caller knows to finish enrolling.
fn complete_mfa_setup(
    state: &CognitoState,
    client_id: &str,
    region: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    let session_id = input["Session"].as_str().unwrap_or("");
    let session = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }

    let setup_done = {
        let mut pool = state.user_pools.get_mut(&session.pool_id).ok_or_else(|| {
            AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
        })?;
        let user = pool.users.get_mut(&session.username).ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?;
        if user.totp_verified {
            user.mfa_enabled = true;
            if user.mfa_preferred.is_none() {
                user.mfa_preferred = Some("SOFTWARE_TOKEN_MFA".to_string());
            }
            true
        } else {
            false
        }
    };

    // Software token not yet verified: the caller must associate and verify it
    // before retrying, so re-issue the same challenge.
    if !setup_done {
        return Ok(mfa_setup_challenge(
            state,
            &session.pool_id,
            &session.username,
        ));
    }

    let pool = state.user_pools.get(&session.pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool.users.get(&session.username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    let result = build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        &session.pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    );
    drop(pool);
    state.mfa_sessions.remove(session_id);
    info!(username = %session.username, "Cognito: MFA_SETUP completed");
    Ok(result)
}

/// Reject a refresh token that was explicitly revoked (RevokeToken) or that
/// predates the user's most recent global sign-out. Shared by REFRESH_TOKEN_AUTH
/// and GetTokensFromRefreshToken so both honour revocation. A token whose issue
/// time cannot be read (legacy format) is treated as stale once the user has
/// signed out, failing closed.
fn ensure_refresh_token_active(
    state: &CognitoState,
    user: &CognitoUser,
    refresh_tok: &str,
) -> Result<(), AwsError> {
    if state.revoked_tokens.revoked.contains_key(refresh_tok) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Refresh Token has been revoked",
        ));
    }
    if let Some(signed_out_at) = user.signed_out_at
        && jwt::refresh_token_issued_at(refresh_tok).is_none_or(|issued| issued < signed_out_at)
    {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Refresh Token has been revoked",
        ));
    }
    Ok(())
}

/// Build the list of GroupRolePair for a user from pool group data.
fn group_role_pairs(pool: &UserPool, user_groups: &[String]) -> Vec<GroupRolePair> {
    user_groups
        .iter()
        .filter_map(|gname| {
            pool.groups.get(gname).map(|g| GroupRolePair {
                group_name: g.group_name.clone(),
                role_arn: g.role_arn.clone(),
                precedence: g.precedence,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn build_auth_result_pub(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
    read_attributes: &[String],
    groups: &[GroupRolePair],
) -> Value {
    let validity = TokenValidity::defaults();
    build_auth_result_with_validity(
        user_sub,
        username,
        region,
        pool_id,
        client_id,
        attributes,
        read_attributes,
        groups,
        &validity,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_auth_result_with_validity(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
    read_attributes: &[String],
    groups: &[GroupRolePair],
    validity: &TokenValidity,
) -> Value {
    build_auth_result_inner(
        user_sub,
        username,
        region,
        pool_id,
        client_id,
        attributes,
        read_attributes,
        groups,
        validity,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_auth_result_inner(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
    read_attributes: &[String],
    groups: &[GroupRolePair],
    validity: &TokenValidity,
    include_refresh: bool,
) -> Value {
    let default_scopes: Vec<String> = vec![
        "openid".to_string(),
        "email".to_string(),
        "profile".to_string(),
    ];
    let id_tok = jwt::id_token(
        user_sub,
        region,
        pool_id,
        client_id,
        username,
        attributes,
        read_attributes,
        &default_scopes,
        None,
        groups,
        None,
        validity.id,
    );
    // The ID token uses `default_scopes` only to gate which attribute claims
    // appear. Access tokens minted by the SDK auth flows carry the fixed scope
    // `aws.cognito.signin.user.admin` (passing an empty slice selects that
    // default), not the hosted-UI OAuth scopes.
    let access_tok = jwt::access_token(
        user_sub,
        region,
        pool_id,
        client_id,
        username,
        &[],
        groups,
        None,
        validity.access,
    );

    let mut auth_result = json!({
        "AccessToken": access_tok,
        "IdToken": id_tok,
        "ExpiresIn": validity.access,
        "TokenType": "Bearer"
    });
    // AWS Cognito only includes RefreshToken on the *initial* auth
    // exchange (USER_PASSWORD_AUTH, USER_SRP_AUTH, etc.) — not on
    // subsequent REFRESH_TOKEN_AUTH calls, where the SPA keeps reusing
    // the refresh token it already has.
    if include_refresh && let Some(obj) = auth_result.as_object_mut() {
        obj.insert(
            "RefreshToken".to_string(),
            Value::String(jwt::refresh_token(user_sub)),
        );
    }
    json!({ "AuthenticationResult": auth_result })
}

#[allow(clippy::too_many_arguments)]
fn build_auth_result_validity(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
    read_attributes: &[String],
    groups: &[GroupRolePair],
    validity: &TokenValidity,
) -> Value {
    build_auth_result_with_validity(
        user_sub,
        username,
        region,
        pool_id,
        client_id,
        attributes,
        read_attributes,
        groups,
        validity,
    )
}

/// Publish a fire-and-forget Lambda trigger event onto the event bus.
fn invoke_trigger(ctx: &RequestContext, trigger_source: &str, lambda_arn: &str, event: &Value) {
    if let Some(ref bus) = ctx.event_bus {
        bus.publish(InternalEvent {
            source: "cognito-idp".to_string(),
            event_type: "cognito:LambdaTrigger".to_string(),
            region: ctx.region.clone(),
            account_id: ctx.account_id.clone(),
            detail: json!({
                "triggerSource": trigger_source,
                "functionArn": lambda_arn,
                "event": event,
            }),
        });
    }
}

/// Decide whether a requested `AuthFlow` is permitted by a client's
/// `ExplicitAuthFlows`.
///
/// An empty `explicit_auth_flows` means "no explicit restriction" and every
/// supported flow is allowed (matching Cognito's permissive default for
/// clients that never set the list). When the list is non-empty, the flow is
/// allowed only if a matching entry is present. Both legacy names
/// (`USER_PASSWORD_AUTH`, `ADMIN_NO_SRP_AUTH`, `CUSTOM_AUTH_FLOW_ONLY`) and
/// the modern `ALLOW_`-prefixed names are honoured.
fn auth_flow_allowed(client: &UserPoolClient, auth_flow: &str) -> bool {
    let flows = &client.explicit_auth_flows;
    // An unset ExplicitAuthFlows defaults to ALLOW_USER_SRP_AUTH +
    // ALLOW_CUSTOM_AUTH + ALLOW_REFRESH_TOKEN_AUTH, matching real Cognito. The
    // password flows must be opted into explicitly, so an empty list does not
    // enable them.
    let has = |name: &str| {
        if flows.is_empty() {
            matches!(
                name,
                "ALLOW_USER_SRP_AUTH" | "ALLOW_CUSTOM_AUTH" | "ALLOW_REFRESH_TOKEN_AUTH"
            )
        } else {
            flows.iter().any(|f| f == name)
        }
    };
    match auth_flow {
        "USER_SRP_AUTH" => has("ALLOW_USER_SRP_AUTH"),
        "USER_PASSWORD_AUTH" => has("ALLOW_USER_PASSWORD_AUTH") || has("USER_PASSWORD_AUTH"),
        // AdminInitiateAuth-only flow; gated by its own ALLOW_ entry or the
        // legacy ADMIN_NO_SRP_AUTH name.
        "ADMIN_USER_PASSWORD_AUTH" | "ADMIN_NO_SRP_AUTH" => {
            has("ALLOW_ADMIN_USER_PASSWORD_AUTH") || has("ADMIN_NO_SRP_AUTH")
        }
        "CUSTOM_AUTH" => has("ALLOW_CUSTOM_AUTH") || has("CUSTOM_AUTH_FLOW_ONLY"),
        "REFRESH_TOKEN_AUTH" | "REFRESH_TOKEN" => has("ALLOW_REFRESH_TOKEN_AUTH"),
        "USER_AUTH" => has("ALLOW_USER_AUTH"),
        // Unknown flow names are left for the downstream match to reject with
        // its own "Unsupported AuthFlow" error.
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// InitiateAuth
// ---------------------------------------------------------------------------

pub fn initiate_auth(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let auth_flow = input["AuthFlow"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AuthFlow is required")
    })?;
    let params = &input["AuthParameters"];

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    // Reject flows the client's ExplicitAuthFlows excludes, before doing any
    // credential work, mirroring Cognito's up-front validation.
    {
        let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
            AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
        })?;
        if let Some(client) = pool.clients.get(client_id)
            && !auth_flow_allowed(client, auth_flow)
        {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Auth flow not enabled for this client: {auth_flow}"),
            ));
        }
    }

    match auth_flow {
        "USER_SRP_AUTH" => start_srp_challenge(state, client_id, &pool_id, params),
        "CUSTOM_AUTH" => start_custom_auth_challenge(
            state,
            client_id,
            &pool_id,
            params,
            &client_metadata(input),
            ctx,
        ),
        "USER_PASSWORD_AUTH" => {
            let raw_username = params["USERNAME"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameterException", "USERNAME is required")
            })?;
            let password = params["PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameterException", "PASSWORD is required")
            })?;
            crate::secret_hash::validate_for_client(
                state,
                client_id,
                params["SECRET_HASH"].as_str(),
                raw_username,
            )?;
            let username = {
                let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                super::users::resolve_username_for_signin(&pool, raw_username).ok_or_else(|| {
                    AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                })?
            };
            let username = username.as_str();

            // Pre-Authentication trigger (fire-and-forget) — read pool with
            // an immutable borrow first to fire the trigger, then drop so we
            // can take a mutable borrow for the lockout bookkeeping below.
            {
                let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                if let Some(arn) = pool.lambda_config.get("PreAuthentication") {
                    let trigger_event = json!({
                        "userPoolId": pool_id,
                        "userName": username,
                        "callerContext": { "clientId": client_id }
                    });
                    invoke_trigger(ctx, "PreAuthentication_Authentication", arn, &trigger_event);
                }
            }

            // Lockout / password check / risk evaluation inside a tight
            // mutable scope so the remainder of the flow keeps its existing
            // immutable borrows.
            {
                let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                let block_action = super::auth_policy::compromised_credentials_action_for(
                    &pool,
                    Some(client_id),
                    "SIGN_IN",
                );
                let compromised = super::auth_policy::is_compromised_password(password);

                let user = pool.users.get_mut(username).ok_or_else(|| {
                    AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                })?;
                if !user.enabled {
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "User is disabled.",
                    ));
                }
                super::auth_policy::check_not_locked(user)?;

                if !crate::password::verify(password, &user.password_hash) {
                    super::auth_policy::record_attempt(user, false);
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, compromised),
                    );
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "Incorrect username or password.",
                    ));
                }

                if compromised && block_action.as_deref() == Some("BLOCK") {
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, true),
                    );
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "Risk-based authentication blocked sign-in: compromised credentials",
                    ));
                }

                super::auth_policy::record_attempt(user, true);
                super::auth_policy::record_auth_event(
                    user,
                    super::auth_policy::build_signin_event(true, compromised),
                );
            }

            let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed.",
                ));
            }

            // RESET_REQUIRED: admin reset password, user must use ForgotPassword
            if user.status == "RESET_REQUIRED" {
                return Err(AwsError::bad_request(
                    "PasswordResetRequiredException",
                    "Password reset required for the user",
                ));
            }

            // FORCE_CHANGE_PASSWORD challenge
            if user.status == "FORCE_CHANGE_PASSWORD" {
                let user_attrs_json = user_attributes_param(user);
                let session_id = issue_challenge_session(state, &pool_id, username);
                info!(username = %username, "Cognito: InitiateAuth -> NEW_PASSWORD_REQUIRED");
                return Ok(json!({
                    "ChallengeName": "NEW_PASSWORD_REQUIRED",
                    "Session": session_id,
                    "ChallengeParameters": {
                        "USER_ID_FOR_SRP": username,
                        "userAttributes": user_attrs_json,
                        "requiredAttributes": "[]"
                    }
                }));
            }

            // Check whether MFA is required
            let mfa_required = pool.mfa_configuration == "ON"
                || (pool.mfa_configuration == "OPTIONAL" && user.mfa_enabled);
            let mfa_eligible = mfa_required
                && (software_factor_enabled(user)
                    || sms_factor_enabled(user)
                    || email_factor_enabled(user));
            // A pool with MfaConfiguration ON forces every user to have MFA, so
            // a user with no usable factor must set one up before sign-in
            // completes rather than receiving tokens.
            let mfa_setup_required = pool.mfa_configuration == "ON";
            // Release the outer read guard before any mutable re-acquire so we
            // never hold two guards on the same pool at once.
            drop(pool);

            if mfa_eligible {
                let challenge = {
                    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                        AwsError::service_not_found(
                            "ResourceNotFoundException",
                            "User pool not found",
                        )
                    })?;
                    let user = pool.users.get_mut(username).ok_or_else(|| {
                        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                    })?;
                    issue_mfa_challenge(user)
                };
                if let Some(challenge) = challenge {
                    info!(username = %username, "Cognito: InitiateAuth -> MFA challenge");
                    return Ok(mfa_challenge_response(state, &pool_id, username, challenge));
                }
            }

            if mfa_setup_required {
                info!(username = %username, "Cognito: InitiateAuth -> MFA_SETUP");
                return Ok(mfa_setup_challenge(state, &pool_id, username));
            }

            let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            // Post-Authentication trigger (fire-and-forget)
            if let Some(arn) = pool.lambda_config.get("PostAuthentication") {
                let trigger_event = json!({
                    "userPoolId": pool_id,
                    "userName": username,
                    "callerContext": { "clientId": client_id }
                });
                invoke_trigger(
                    ctx,
                    "PostAuthentication_Authentication",
                    arn,
                    &trigger_event,
                );
            }

            let pairs = group_role_pairs(&pool, &user.groups);
            let validity = pool
                .clients
                .get(client_id)
                .map(TokenValidity::from_client)
                .unwrap_or_else(TokenValidity::defaults);
            let result = build_auth_result_validity(
                &user.sub,
                username,
                &ctx.region,
                &pool_id,
                client_id,
                &user.attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
            );

            info!(username = %username, "Cognito: InitiateAuth success");
            Ok(result)
        }
        "REFRESH_TOKEN_AUTH" | "REFRESH_TOKEN" => {
            refresh_token_auth(state, &pool_id, client_id, params, &ctx.region)
        }
        flow => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unsupported AuthFlow: {flow}"),
        )),
    }
}

/// Shared REFRESH_TOKEN_AUTH handler for both InitiateAuth and
/// AdminInitiateAuth. Resolves the user from our opaque
/// `refresh-{sub}.{ts}.{uuid}` token, honours revocation, and reissues
/// access/id tokens without a new refresh token (matching AWS).
fn refresh_token_auth(
    state: &CognitoState,
    pool_id: &str,
    client_id: &str,
    params: &Value,
    region: &str,
) -> Result<Value, AwsError> {
    let refresh_tok = params["REFRESH_TOKEN"].as_str().ok_or_else(|| {
        AwsError::bad_request("NotAuthorizedException", "Refresh token is missing.")
    })?;

    let sub = refresh_tok
        .strip_prefix("refresh-")
        .and_then(|s| s.split('.').next())
        .unwrap_or("");

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool
        .users
        .values()
        .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Refresh Token."))?;

    if !user.enabled {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "User is disabled.",
        ));
    }

    // Cognito accepts SECRET_HASH on REFRESH_TOKEN_AUTH computed with either
    // the original username or the sub. Validate against the resolved client.
    if let Some(client) = pool.clients.get(client_id) {
        crate::secret_hash::validate_any_username(
            client,
            params["SECRET_HASH"].as_str(),
            &[user.username.as_str(), user.sub.as_str()],
            client_id,
        )?;
    }

    ensure_refresh_token_active(state, user, refresh_tok)?;

    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    // include_refresh=false: AWS doesn't reissue a RefreshToken on
    // REFRESH_TOKEN_AUTH; the client keeps the original one.
    Ok(build_auth_result_inner(
        &user.sub,
        &user.username,
        region,
        pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
        false,
    ))
}

// ---------------------------------------------------------------------------
// AdminInitiateAuth
// ---------------------------------------------------------------------------

pub fn admin_initiate_auth(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let auth_flow = input["AuthFlow"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AuthFlow is required")
    })?;
    let params = &input["AuthParameters"];

    {
        let pool = state.user_pools.get(pool_id).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("User pool {pool_id} does not exist."),
            )
        })?;

        let client = pool.clients.get(client_id).ok_or_else(|| {
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("User pool client {client_id} does not exist."),
            )
        })?;

        // Reject flows excluded by ExplicitAuthFlows. AdminInitiateAuth may
        // additionally use ADMIN_USER_PASSWORD_AUTH, gated by
        // ALLOW_ADMIN_USER_PASSWORD_AUTH.
        if !auth_flow_allowed(client, auth_flow) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Auth flow not enabled for this client: {auth_flow}"),
            ));
        }
    }

    match auth_flow {
        // USER_SRP_AUTH on the admin path returns a PASSWORD_VERIFIER
        // challenge just like the public path; the caller sends SRP_A, not a
        // password.
        "USER_SRP_AUTH" => start_srp_challenge(state, client_id, pool_id, params),
        "CUSTOM_AUTH" => start_custom_auth_challenge(
            state,
            client_id,
            pool_id,
            params,
            &client_metadata(input),
            ctx,
        ),
        "REFRESH_TOKEN_AUTH" | "REFRESH_TOKEN" => {
            refresh_token_auth(state, pool_id, client_id, params, &ctx.region)
        }
        // Real Cognito rejects the non-admin USER_PASSWORD_AUTH on the admin
        // API; admins must use ADMIN_USER_PASSWORD_AUTH (or the legacy
        // ADMIN_NO_SRP_AUTH alias).
        "USER_PASSWORD_AUTH" => Err(AwsError::bad_request(
            "InvalidParameterException",
            "Initiate Auth method not supported",
        )),
        "ADMIN_USER_PASSWORD_AUTH" | "ADMIN_NO_SRP_AUTH" => {
            let raw_username = params["USERNAME"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameterException", "USERNAME is required")
            })?;
            let password = params["PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameterException", "PASSWORD is required")
            })?;
            crate::secret_hash::validate_for_client(
                state,
                client_id,
                params["SECRET_HASH"].as_str(),
                raw_username,
            )?;
            let username = {
                let pool = state.user_pools.get(pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                super::users::resolve_username_for_signin(&pool, raw_username).ok_or_else(|| {
                    AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                })?
            };
            let username = username.as_str();

            // Pre-Authentication trigger (fire-and-forget) — separate
            // immutable scope so we can take a mutable borrow below.
            {
                let pool = state.user_pools.get(pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                if let Some(arn) = pool.lambda_config.get("PreAuthentication") {
                    let trigger_event = json!({
                        "userPoolId": pool_id,
                        "userName": username,
                        "callerContext": { "clientId": client_id }
                    });
                    invoke_trigger(ctx, "PreAuthentication_Authentication", arn, &trigger_event);
                }
            }

            // Lockout / password check / risk evaluation inside a tight
            // mutable scope; the rest of the flow re-acquires an immutable
            // borrow without overlapping with the &mut user.
            {
                let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
                    AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
                })?;
                let block_action = super::auth_policy::compromised_credentials_action_for(
                    &pool,
                    Some(client_id),
                    "SIGN_IN",
                );
                let compromised = super::auth_policy::is_compromised_password(password);

                let user = pool.users.get_mut(username).ok_or_else(|| {
                    AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                })?;
                if !user.enabled {
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "User is disabled.",
                    ));
                }
                super::auth_policy::check_not_locked(user)?;

                if !crate::password::verify(password, &user.password_hash) {
                    super::auth_policy::record_attempt(user, false);
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, compromised),
                    );
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "Incorrect username or password.",
                    ));
                }

                if compromised && block_action.as_deref() == Some("BLOCK") {
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, true),
                    );
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "Risk-based authentication blocked sign-in: compromised credentials",
                    ));
                }

                super::auth_policy::record_attempt(user, true);
                super::auth_policy::record_auth_event(
                    user,
                    super::auth_policy::build_signin_event(true, compromised),
                );
            }

            let pool = state.user_pools.get(pool_id).ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed.",
                ));
            }

            if user.status == "RESET_REQUIRED" {
                return Err(AwsError::bad_request(
                    "PasswordResetRequiredException",
                    "Password reset required for the user",
                ));
            }

            // FORCE_CHANGE_PASSWORD challenge
            if user.status == "FORCE_CHANGE_PASSWORD" {
                let user_attrs_json = user_attributes_param(user);
                let session_id = issue_challenge_session(state, pool_id, username);
                info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth -> NEW_PASSWORD_REQUIRED");
                return Ok(json!({
                    "ChallengeName": "NEW_PASSWORD_REQUIRED",
                    "Session": session_id,
                    "ChallengeParameters": {
                        "USER_ID_FOR_SRP": username,
                        "userAttributes": user_attrs_json,
                        "requiredAttributes": "[]"
                    }
                }));
            }

            // Check whether MFA is required
            let mfa_required = pool.mfa_configuration == "ON"
                || (pool.mfa_configuration == "OPTIONAL" && user.mfa_enabled);
            let mfa_eligible = mfa_required
                && (software_factor_enabled(user)
                    || sms_factor_enabled(user)
                    || email_factor_enabled(user));
            // MfaConfiguration ON forces MFA: a user with no usable factor must
            // set one up before sign-in completes rather than receiving tokens.
            let mfa_setup_required = pool.mfa_configuration == "ON";
            // Release the outer read guard before any mutable re-acquire so we
            // never hold two guards on the same pool at once.
            drop(pool);

            if mfa_eligible {
                let challenge = {
                    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
                        AwsError::service_not_found(
                            "ResourceNotFoundException",
                            "User pool not found",
                        )
                    })?;
                    let user = pool.users.get_mut(username).ok_or_else(|| {
                        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                    })?;
                    issue_mfa_challenge(user)
                };
                if let Some(challenge) = challenge {
                    info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth -> MFA challenge");
                    return Ok(mfa_challenge_response(state, pool_id, username, challenge));
                }
            }

            if mfa_setup_required {
                info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth -> MFA_SETUP");
                return Ok(mfa_setup_challenge(state, pool_id, username));
            }

            let pool = state.user_pools.get(pool_id).ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            // Post-Authentication trigger (fire-and-forget)
            if let Some(arn) = pool.lambda_config.get("PostAuthentication") {
                let trigger_event = json!({
                    "userPoolId": pool_id,
                    "userName": username,
                    "callerContext": { "clientId": client_id }
                });
                invoke_trigger(
                    ctx,
                    "PostAuthentication_Authentication",
                    arn,
                    &trigger_event,
                );
            }

            let pairs = group_role_pairs(&pool, &user.groups);
            let validity = pool
                .clients
                .get(client_id)
                .map(TokenValidity::from_client)
                .unwrap_or_else(TokenValidity::defaults);
            let result = build_auth_result_validity(
                &user.sub,
                username,
                &ctx.region,
                pool_id,
                client_id,
                &user.attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
            );

            info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth success");
            Ok(result)
        }
        flow => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unsupported AuthFlow: {flow}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// RespondToAuthChallenge
// ---------------------------------------------------------------------------

pub fn respond_to_auth_challenge(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let challenge_name = input["ChallengeName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ChallengeName is required")
    })?;
    let responses = &input["ChallengeResponses"];

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    // Cognito clients with a secret carry SECRET_HASH inside ChallengeResponses
    // for every challenge response. The username can come either from the
    // explicit USERNAME response field or from the linked MFA session, so we
    // try both.
    let challenge_username = responses["USERNAME"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            input["Session"].as_str().and_then(|s| {
                state
                    .mfa_sessions
                    .get(s)
                    .map(|e| e.value().username.clone())
            })
        })
        .unwrap_or_default();
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        responses["SECRET_HASH"].as_str(),
        &challenge_username,
    )?;

    match challenge_name {
        "NEW_PASSWORD_REQUIRED" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            let username = resolve_challenge_session(
                state,
                &pool_id,
                session_id,
                responses["USERNAME"].as_str(),
            )?;
            let username = username.as_str();
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "NEW_PASSWORD is required in ChallengeResponses",
                )
            })?;

            let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let policy = pool.policies.clone();
            let user = pool.users.get_mut(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            super::auth_policy::validate_password(&policy, new_password)?;
            user.password_hash = crate::password::hash(new_password)?;
            let (s, v) = crate::password::srp_material(&pool_id, username, new_password);
            user.srp_salt = Some(s);
            user.srp_verifier = Some(v);
            user.status = "CONFIRMED".to_string();

            // Collect needed values before releasing the mutable borrow on users.
            let user_sub = user.sub.clone();
            let user_attributes = user.attributes.clone();
            let user_groups = user.groups.clone();

            let pairs = group_role_pairs(&pool, &user_groups);
            let validity = pool
                .clients
                .get(client_id)
                .map(TokenValidity::from_client)
                .unwrap_or_else(TokenValidity::defaults);
            let result = build_auth_result_validity(
                &user_sub,
                username,
                &ctx.region,
                &pool_id,
                client_id,
                &user_attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
            );

            drop(pool);
            state.mfa_sessions.remove(session_id);
            info!(username = %username, "Cognito: RespondToAuthChallenge NEW_PASSWORD_REQUIRED success");
            Ok(result)
        }
        "SOFTWARE_TOKEN_MFA" => complete_software_token_mfa(state, client_id, &ctx.region, input),
        "SMS_MFA" => verify_code_mfa(
            state,
            client_id,
            &ctx.region,
            input,
            "SMS_MFA_CODE",
            SMS_MFA_KEY,
        ),
        "EMAIL_OTP" => verify_code_mfa(
            state,
            client_id,
            &ctx.region,
            input,
            "EMAIL_OTP_CODE",
            EMAIL_OTP_KEY,
        ),
        "SELECT_MFA_TYPE" => {
            // The user picks a factor via ANSWER; we re-issue the concrete
            // challenge for that factor, reusing the same session.
            let session_id = input["Session"].as_str().unwrap_or("");
            let answer = responses["ANSWER"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "ChallengeResponses.ANSWER is required",
                )
            })?;

            let session_meta = state
                .mfa_sessions
                .get(session_id)
                .map(|e| e.value().clone())
                .ok_or_else(|| {
                    AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
                })?;
            if !session_still_valid(session_meta.issued_at) {
                state.mfa_sessions.remove(session_id);
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Invalid session for the user, session is expired.",
                ));
            }

            let challenge = match answer {
                "SMS_MFA" => {
                    let mut pool =
                        state
                            .user_pools
                            .get_mut(&session_meta.pool_id)
                            .ok_or_else(|| {
                                AwsError::service_not_found(
                                    "ResourceNotFoundException",
                                    "Pool not found",
                                )
                            })?;
                    let user = pool.users.get_mut(&session_meta.username).ok_or_else(|| {
                        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
                    })?;
                    let code = generate_mfa_code();
                    let destination = user
                        .attributes
                        .get("phone_number")
                        .map(|p| mask_destination(p))
                        .unwrap_or_else(|| "+*******".to_string());
                    user.pending_verifications
                        .insert(SMS_MFA_KEY.to_string(), code);
                    user.pending_verifications_issued
                        .insert(SMS_MFA_KEY.to_string(), now_epoch());
                    MfaChallenge::Sms { destination }
                }
                "SOFTWARE_TOKEN_MFA" => MfaChallenge::SoftwareToken,
                other => {
                    return Err(AwsError::bad_request(
                        "InvalidParameterException",
                        format!("Unsupported MFA selection: {other}"),
                    ));
                }
            };

            // Reuse the existing session id so the chosen-factor response can
            // resolve back to the same user.
            let response = mfa_select_followup(session_id, &session_meta.username, challenge);
            info!(username = %session_meta.username, selection = %answer, "Cognito: SELECT_MFA_TYPE");
            Ok(response)
        }
        "MFA_SETUP" => complete_mfa_setup(state, client_id, &ctx.region, input),
        "PASSWORD_VERIFIER" => verify_srp_password(state, &pool_id, client_id, &ctx.region, input),
        "CUSTOM_CHALLENGE" => {
            verify_custom_auth_response(state, &pool_id, client_id, &ctx.region, input, ctx)
        }
        name => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unsupported ChallengeName: {name}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// AdminRespondToAuthChallenge
// ---------------------------------------------------------------------------

pub fn admin_respond_to_auth_challenge(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let challenge_name = input["ChallengeName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ChallengeName is required")
    })?;
    let responses = &input["ChallengeResponses"];

    match challenge_name {
        "NEW_PASSWORD_REQUIRED" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            let username = resolve_challenge_session(
                state,
                pool_id,
                session_id,
                responses["USERNAME"].as_str(),
            )?;
            let username = username.as_str();
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "NEW_PASSWORD is required in ChallengeResponses",
                )
            })?;

            let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
                AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("User pool {pool_id} does not exist."),
                )
            })?;

            if !pool.clients.contains_key(client_id) {
                return Err(AwsError::service_not_found(
                    "ResourceNotFoundException",
                    format!("User pool client {client_id} does not exist."),
                ));
            }

            let policy = pool.policies.clone();
            let user = pool.users.get_mut(username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;

            super::auth_policy::validate_password(&policy, new_password)?;
            user.password_hash = crate::password::hash(new_password)?;
            let (s, v) = crate::password::srp_material(pool_id, username, new_password);
            user.srp_salt = Some(s);
            user.srp_verifier = Some(v);
            user.status = "CONFIRMED".to_string();

            // Collect needed values before releasing the mutable borrow on users.
            let user_sub = user.sub.clone();
            let user_attributes = user.attributes.clone();
            let user_groups = user.groups.clone();

            let pairs = group_role_pairs(&pool, &user_groups);
            let validity = pool
                .clients
                .get(client_id)
                .map(TokenValidity::from_client)
                .unwrap_or_else(TokenValidity::defaults);
            let result = build_auth_result_validity(
                &user_sub,
                username,
                &ctx.region,
                pool_id,
                client_id,
                &user_attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
            );

            drop(pool);
            state.mfa_sessions.remove(session_id);
            info!(username = %username, pool_id = %pool_id, "Cognito: AdminRespondToAuthChallenge NEW_PASSWORD_REQUIRED success");
            Ok(result)
        }
        "SOFTWARE_TOKEN_MFA" => complete_software_token_mfa(state, client_id, &ctx.region, input),
        "SMS_MFA" => verify_code_mfa(
            state,
            client_id,
            &ctx.region,
            input,
            "SMS_MFA_CODE",
            SMS_MFA_KEY,
        ),
        "EMAIL_OTP" => verify_code_mfa(
            state,
            client_id,
            &ctx.region,
            input,
            "EMAIL_OTP_CODE",
            EMAIL_OTP_KEY,
        ),
        "MFA_SETUP" => complete_mfa_setup(state, client_id, &ctx.region, input),
        "PASSWORD_VERIFIER" => verify_srp_password(state, pool_id, client_id, &ctx.region, input),
        "CUSTOM_CHALLENGE" => {
            verify_custom_auth_response(state, pool_id, client_id, &ctx.region, input, ctx)
        }
        name => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unsupported ChallengeName: {name}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// GetTokensFromRefreshToken
// ---------------------------------------------------------------------------

pub fn get_tokens_from_refresh_token(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let refresh_tok = input["RefreshToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "RefreshToken is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    // Extract sub from our opaque refresh token format: "refresh-{sub}.{uuid}"
    let sub = refresh_tok
        .strip_prefix("refresh-")
        .and_then(|s| s.split('.').next())
        .unwrap_or("unknown");

    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;
    let user = pool
        .users
        .values()
        .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
        .ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User not found for refresh token")
        })?;
    ensure_refresh_token_active(state, user, refresh_tok)?;

    let pairs = group_role_pairs(&pool, &user.groups);
    Ok(build_auth_result_pub(
        &user.sub,
        &user.username,
        &ctx.region,
        &pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
    ))
}

// ---------------------------------------------------------------------------
// GetUserAuthFactors
// ---------------------------------------------------------------------------

pub fn get_user_auth_factors(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    let username = crate::jwt::extract_username_from_access_token(token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

    // Find pool containing this user
    for pool_ref in state.user_pools.iter() {
        if let Some(user) = pool_ref.users.get(&username) {
            let mut factors = vec!["PASSWORD".to_string()];
            if user.totp_verified {
                factors.push("SOFTWARE_TOKEN_MFA".to_string());
            }
            return Ok(json!({
                "Username": username,
                "ConfiguredUserAuthFactors": factors
            }));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
    ))
}

// ---------------------------------------------------------------------------
// SRP6a auth flow (USER_SRP_AUTH -> PASSWORD_VERIFIER challenge)
// ---------------------------------------------------------------------------

/// Phase 1 of USER_SRP_AUTH: the client has sent SRP_A and we respond with a
/// PASSWORD_VERIFIER challenge carrying the per-user salt and a fresh
/// server-side ephemeral key B. The session is stashed so we can finish the
/// proof in `verify_srp_password` once the client comes back.
fn start_srp_challenge(
    state: &CognitoState,
    client_id: &str,
    pool_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, AwsError> {
    use num_bigint::BigUint;
    use num_traits::Num;

    let raw_username = params["USERNAME"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "USERNAME is required")
    })?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        params["SECRET_HASH"].as_str(),
        raw_username,
    )?;
    let srp_a_hex = params["SRP_A"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "SRP_A is required"))?;
    if BigUint::from_str_radix(srp_a_hex, 16).is_err() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "SRP_A is not a hex-encoded big integer",
        ));
    }

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let resolved_username = super::users::resolve_username_for_signin(&pool, raw_username)
        .ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?;

    let user = pool.users.get(&resolved_username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    if !user.enabled {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "User is disabled.",
        ));
    }
    let salt_hex = user.srp_salt.clone().ok_or_else(|| {
        AwsError::bad_request(
            "NotAuthorizedException",
            "User has no SRP material; ask the admin to reset the password",
        )
    })?;
    let verifier_hex = user.srp_verifier.clone().ok_or_else(|| {
        AwsError::bad_request("NotAuthorizedException", "User has no SRP verifier")
    })?;
    let verifier_big = BigUint::from_str_radix(&verifier_hex, 16)
        .map_err(|_| crate::error::internal_error("Stored SRP verifier is not valid hex"))?;

    let (b_priv, b_pub) = crate::srp::server_keys(&verifier_big);
    let b_priv_hex = b_priv.to_str_radix(16);
    let b_pub_hex = b_pub.to_str_radix(16);
    let secret_block = crate::srp::random_secret_block_b64();

    let session_id = uuid::Uuid::new_v4().to_string();
    state.srp_sessions.insert(
        session_id.clone(),
        crate::state::SrpSession {
            pool_id: pool_id.to_string(),
            username: resolved_username.clone(),
            client_id: client_id.to_string(),
            b_priv_hex,
            b_pub_hex: b_pub_hex.clone(),
            salt_hex: salt_hex.clone(),
            secret_block_b64: secret_block.clone(),
            issued_at: now_epoch(),
        },
    );

    Ok(json!({
        "ChallengeName": "PASSWORD_VERIFIER",
        "Session": session_id,
        "ChallengeParameters": {
            "SALT": salt_hex,
            "SRP_B": b_pub_hex,
            "SECRET_BLOCK": secret_block,
            "USERNAME": resolved_username,
            "USER_ID_FOR_SRP": resolved_username,
        }
    }))
}

/// Phase 2 of USER_SRP_AUTH: the client returns its proof M1 and we verify
/// it against the value computed from `(b, B, A, v)`. On success we issue
/// the regular auth-result tokens.
fn verify_srp_password(
    state: &CognitoState,
    pool_id: &str,
    client_id: &str,
    region: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, AwsError> {
    use base64::Engine as _;
    use num_bigint::BigUint;
    use num_traits::Num;

    let session_id = input["Session"].as_str().unwrap_or("");
    let session = state
        .srp_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if session.client_id != client_id || session.pool_id != pool_id {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Session does not match client or pool",
        ));
    }
    if !session_still_valid(session.issued_at) {
        state.srp_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }

    let resp = &input["ChallengeResponses"];
    let username = resp["USERNAME"].as_str().unwrap_or(&session.username);
    let secret_block_b64 = resp["PASSWORD_CLAIM_SECRET_BLOCK"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "PASSWORD_CLAIM_SECRET_BLOCK is required",
            )
        })?;
    if secret_block_b64 != session.secret_block_b64 {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Incorrect username or password.",
        ));
    }
    let timestamp = resp["TIMESTAMP"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "TIMESTAMP is required")
    })?;
    let signature_b64 = resp["PASSWORD_CLAIM_SIGNATURE"].as_str().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "PASSWORD_CLAIM_SIGNATURE is required",
        )
    })?;
    let provided_sig = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| {
            AwsError::bad_request(
                "InvalidParameterException",
                "PASSWORD_CLAIM_SIGNATURE is not valid base64",
            )
        })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool.users.get(&session.username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let verifier_hex = user.srp_verifier.clone().ok_or_else(|| {
        crate::error::internal_error("User has no SRP verifier; cannot complete PASSWORD_VERIFIER")
    })?;
    let verifier_big = BigUint::from_str_radix(&verifier_hex, 16)
        .map_err(|_| crate::error::internal_error("Stored SRP verifier is not valid hex"))?;
    let b_priv = BigUint::from_str_radix(&session.b_priv_hex, 16)
        .map_err(|_| crate::error::internal_error("Stored SRP b is not valid hex"))?;
    let b_pub = BigUint::from_str_radix(&session.b_pub_hex, 16)
        .map_err(|_| crate::error::internal_error("Stored SRP B is not valid hex"))?;

    // The client sends SRP_A (its public ephemeral) inside the challenge
    // responses on the second leg too, since the server needs both A and B
    // to derive K.
    let a_hex = resp["SRP_A"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "SRP_A is required"))?;
    let a_pub = BigUint::from_str_radix(a_hex, 16).map_err(|_| {
        AwsError::bad_request("InvalidParameterException", "SRP_A is not valid hex")
    })?;

    let k_session =
        crate::srp::derive_k(&a_pub, &b_pub, &b_priv, &verifier_big).ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "SRP key derivation failed")
        })?;

    let pool_short = crate::password::pool_short_name(pool_id);
    let expected = crate::srp::expected_m1(
        &k_session,
        pool_short,
        username,
        secret_block_b64.as_bytes(),
        timestamp,
    );
    if !crate::srp::ct_eq(&expected, &provided_sig) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Incorrect username or password.",
        ));
    }

    // SRP succeeded; mint tokens. Drop the session so it can't be replayed.
    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    let result = build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    );
    drop(pool);
    state.srp_sessions.remove(session_id);
    info!(username = %session.username, "Cognito: SRP PASSWORD_VERIFIER success");
    Ok(result)
}

// ---------------------------------------------------------------------------
// CUSTOM_AUTH flow
// ---------------------------------------------------------------------------

/// Maximum CUSTOM_AUTH rounds before the flow is abandoned, matching
/// Cognito's documented three-attempt cap.
const MAX_CUSTOM_AUTH_ROUNDS: usize = 3;

/// A user's attributes as the `{name: value}` map Cognito puts in trigger
/// events.
fn user_attributes_object(user: &CognitoUser) -> Value {
    Value::Object(
        user.attributes
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

/// Build the `request.session` list a CUSTOM_AUTH trigger receives: one entry
/// per completed/pending round.
fn custom_auth_session_list(rounds: &[CustomAuthRound]) -> Value {
    Value::Array(
        rounds
            .iter()
            .map(|r| {
                json!({
                    "challengeName": "CUSTOM_CHALLENGE",
                    "challengeMetadata": r.challenge_metadata,
                    "challengeResult": r.challenge_result.unwrap_or(false),
                })
            })
            .collect(),
    )
}

/// Read top-level `ClientMetadata` (a string map) from the request, defaulting
/// to an empty object. Threaded into Create/Verify trigger events.
fn client_metadata(input: &Value) -> Value {
    match input.get("ClientMetadata") {
        Some(v) if v.is_object() => v.clone(),
        _ => json!({}),
    }
}

/// Invoke a CUSTOM_AUTH trigger synchronously, mapping a missing function or
/// any transport/shape failure to `InvalidLambdaResponseException` as Cognito
/// does. `trigger` is the bare trigger name used in messages.
fn invoke_custom_auth_trigger(
    invoker: &dyn LambdaInvoker,
    trigger: &str,
    arn: &str,
    account_id: &str,
    region: &str,
    event: &Value,
) -> Result<Value, AwsError> {
    let resp = invoker
        .invoke(arn, event, account_id, region)
        .map_err(|e| {
            let msg = if e.code == "ResourceNotFoundException" {
                format!("{trigger} Lambda not found: {arn}")
            } else {
                format!("{trigger} invocation failed: {}", e.message)
            };
            AwsError::bad_request("InvalidLambdaResponseException", msg)
        })?;
    if !resp.is_object() {
        return Err(AwsError::bad_request(
            "InvalidLambdaResponseException",
            format!("{trigger} returned an invalid response"),
        ));
    }
    Ok(resp)
}

/// Build the standard trigger event envelope shared by the three CUSTOM_AUTH
/// triggers.
fn custom_auth_event(
    trigger_source: &str,
    region: &str,
    pool_id: &str,
    username: &str,
    client_id: &str,
    user: &CognitoUser,
    request_extra: Value,
) -> Value {
    let mut request = json!({
        "userAttributes": user_attributes_object(user),
        "userNotFound": false,
    });
    if let (Some(obj), Some(extra)) = (request.as_object_mut(), request_extra.as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    json!({
        "version": "1",
        "triggerSource": trigger_source,
        "region": region,
        "userPoolId": pool_id,
        "userName": username,
        "callerContext": { "awsSdkVersion": "aws-sdk-unknown", "clientId": client_id },
        "request": request,
        "response": {},
    })
}

/// Run CreateAuthChallenge for the next CUSTOM_CHALLENGE round and return the
/// `(public_params, private_params, challenge_metadata)` it produced. With no
/// CreateAuthChallenge Lambda configured, defaults are used (mirroring AWS,
/// which still issues a CUSTOM_CHALLENGE with a default metadata).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn run_create_auth_challenge(
    invoker: &dyn LambdaInvoker,
    pool: &UserPool,
    account: &str,
    region: &str,
    pool_id: &str,
    username: &str,
    client_id: &str,
    user: &CognitoUser,
    rounds: &[CustomAuthRound],
    metadata: &Value,
) -> Result<
    (
        serde_json::Map<String, Value>,
        HashMap<String, String>,
        Option<String>,
    ),
    AwsError,
> {
    let Some(arn) = pool.lambda_config.get("CreateAuthChallenge") else {
        return Ok((serde_json::Map::new(), HashMap::new(), None));
    };
    let mut extra = json!({
        "challengeName": "CUSTOM_CHALLENGE",
        "session": custom_auth_session_list(rounds),
        "clientMetadata": metadata,
    });
    if let Some(obj) = extra.as_object_mut() {
        obj.insert("userAttributes".to_string(), user_attributes_object(user));
    }
    let event = custom_auth_event(
        "CreateAuthChallenge_Authentication",
        region,
        pool_id,
        username,
        client_id,
        user,
        extra,
    );
    let resp =
        invoke_custom_auth_trigger(invoker, "CreateAuthChallenge", arn, account, region, &event)?;
    let response = &resp["response"];
    let public = response["publicChallengeParameters"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let private = response["privateChallengeParameters"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let challenge_metadata = response["challengeMetadata"].as_str().map(String::from);
    Ok((public, private, challenge_metadata))
}

/// Handle an `InitiateAuth(CUSTOM_AUTH)`. When a DefineAuthChallenge Lambda is
/// configured and a synchronous invoker is available, run the real
/// Define -> Create state machine driven by the Lambda responses. Otherwise
/// fall back to the fixture-backed single round (used in unit tests and pools
/// with no triggers), which fails closed unless a fixture is configured.
fn start_custom_auth_challenge(
    state: &CognitoState,
    client_id: &str,
    pool_id: &str,
    params: &serde_json::Value,
    client_metadata: &Value,
    ctx: &RequestContext,
) -> Result<serde_json::Value, AwsError> {
    let raw_username = params["USERNAME"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "USERNAME is required")
    })?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        params["SECRET_HASH"].as_str(),
        raw_username,
    )?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let resolved_username = super::users::resolve_username_for_signin(&pool, raw_username)
        .ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?;
    if let Some(user) = pool.users.get(&resolved_username)
        && !user.enabled
    {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "User is disabled.",
        ));
    }

    let define_arn = pool.lambda_config.get("DefineAuthChallenge").cloned();
    let invoker = state.lambda_invoker.get().cloned();

    // Lambda-driven path: a DefineAuthChallenge trigger plus a real invoker.
    if let (Some(define_arn), Some(invoker)) = (define_arn, invoker) {
        return start_custom_auth_lambda(
            state,
            &pool,
            pool_id,
            client_id,
            &resolved_username,
            &define_arn,
            invoker.as_ref(),
            client_metadata,
            ctx,
        );
    }

    // Fixture path: emit the CUSTOM_CHALLENGE backed by the pool fixture and
    // still publish the fire-and-forget triggers for observers.
    let challenge_params = pool.custom_auth_challenge_parameters.clone();
    if let Some(arn) = pool.lambda_config.get("DefineAuthChallenge") {
        invoke_trigger(
            ctx,
            "DefineAuthChallenge_Authentication",
            arn,
            &json!({
                "userPoolId": pool_id,
                "userName": resolved_username,
                "callerContext": { "clientId": client_id },
                "request": { "session": [], "userAttributes": {} }
            }),
        );
    }
    if let Some(arn) = pool.lambda_config.get("CreateAuthChallenge") {
        invoke_trigger(
            ctx,
            "CreateAuthChallenge_Authentication",
            arn,
            &json!({
                "userPoolId": pool_id,
                "userName": resolved_username,
                "callerContext": { "clientId": client_id },
                "request": { "challengeName": "CUSTOM_CHALLENGE", "session": [] }
            }),
        );
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    state.mfa_sessions.insert(
        session_id.clone(),
        crate::state::MfaSession {
            pool_id: pool_id.to_string(),
            username: resolved_username.clone(),
            issued_at: now_epoch(),
        },
    );

    let mut params_json = serde_json::Map::new();
    params_json.insert(
        "USERNAME".to_string(),
        Value::String(resolved_username.clone()),
    );
    for (k, v) in challenge_params {
        params_json.insert(k, Value::String(v));
    }

    info!(username = %resolved_username, "Cognito: CUSTOM_AUTH -> CUSTOM_CHALLENGE");
    Ok(json!({
        "ChallengeName": "CUSTOM_CHALLENGE",
        "Session": session_id,
        "ChallengeParameters": params_json,
    }))
}

/// Lambda-driven InitiateAuth(CUSTOM_AUTH): DefineAuthChallenge decides the
/// next step (issue tokens, fail, or a CUSTOM_CHALLENGE round), then
/// CreateAuthChallenge supplies the round parameters.
#[allow(clippy::too_many_arguments)]
fn start_custom_auth_lambda(
    state: &CognitoState,
    pool: &UserPool,
    pool_id: &str,
    client_id: &str,
    username: &str,
    define_arn: &str,
    invoker: &dyn LambdaInvoker,
    client_metadata: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let user = pool
        .users
        .get(username)
        .ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?
        .clone();
    let region = &ctx.region;
    let account = &ctx.account_id;

    let define_event = custom_auth_event(
        "DefineAuthChallenge_Authentication",
        region,
        pool_id,
        username,
        client_id,
        &user,
        json!({ "session": custom_auth_session_list(&[]) }),
    );
    let define = invoke_custom_auth_trigger(
        invoker,
        "DefineAuthChallenge",
        define_arn,
        account,
        region,
        &define_event,
    )?;
    let response = &define["response"];

    if response["failAuthentication"].as_bool() == Some(true) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Authentication failed",
        ));
    }
    if response["issueTokens"].as_bool() == Some(true) {
        // Zero-round bypass: tokens issued without any challenge.
        return build_custom_auth_tokens(pool, region, pool_id, client_id, &user);
    }
    if response["challengeName"].as_str() != Some("CUSTOM_CHALLENGE") {
        return Err(AwsError::bad_request(
            "InvalidLambdaResponseException",
            "DefineAuthChallenge response invalid",
        ));
    }

    let (public, private, metadata) = run_create_auth_challenge(
        invoker,
        pool,
        account,
        region,
        pool_id,
        username,
        client_id,
        &user,
        &[],
        client_metadata,
    )?;

    let session_id = uuid::Uuid::new_v4().to_string();
    state.custom_auth_sessions.insert(
        session_id.clone(),
        CustomAuthSession {
            pool_id: pool_id.to_string(),
            client_id: client_id.to_string(),
            username: username.to_string(),
            rounds: vec![CustomAuthRound {
                challenge_metadata: metadata,
                challenge_result: None,
            }],
            private_params: private,
            issued_at: now_epoch(),
        },
    );

    let mut params_json = public;
    params_json
        .entry("USERNAME".to_string())
        .or_insert_with(|| Value::String(username.to_string()));
    info!(username = %username, "Cognito: CUSTOM_AUTH (lambda) -> CUSTOM_CHALLENGE");
    Ok(json!({
        "ChallengeName": "CUSTOM_CHALLENGE",
        "Session": session_id,
        "ChallengeParameters": params_json,
    }))
}

/// Mint the final AuthenticationResult for a completed CUSTOM_AUTH flow.
fn build_custom_auth_tokens(
    pool: &UserPool,
    region: &str,
    pool_id: &str,
    client_id: &str,
    user: &CognitoUser,
) -> Result<Value, AwsError> {
    let pairs = group_role_pairs(pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    Ok(build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    ))
}

/// Handle a `RespondToAuthChallenge(CUSTOM_CHALLENGE)`. Real Cognito calls the
/// VerifyAuthChallengeResponse Lambda to decide if `ANSWER` is correct. awsim
/// approximates this: it compares against the pool's
/// `custom_auth_expected_answer` fixture when set. When neither a fixture nor a
/// VerifyAuthChallengeResponse Lambda is configured there is no way to validate
/// the answer, so the flow fails closed rather than minting tokens for any
/// non-empty answer (which would make a default pool
/// passwordless-auth-as-anyone). The Lambda trigger is emitted as a
/// fire-and-forget event; synchronous Lambda-driven evaluation is not yet
/// modelled.
fn verify_custom_auth_response(
    state: &CognitoState,
    pool_id: &str,
    client_id: &str,
    region: &str,
    input: &serde_json::Value,
    ctx: &RequestContext,
) -> Result<serde_json::Value, AwsError> {
    let session_id = input["Session"].as_str().unwrap_or("");

    // Lambda-driven path: the session was created by start_custom_auth_lambda.
    if state.custom_auth_sessions.contains_key(session_id) {
        return verify_custom_auth_lambda(state, pool_id, client_id, session_id, input, ctx);
    }

    let session = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }
    if session.pool_id != pool_id {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Session does not match pool",
        ));
    }

    let resp = &input["ChallengeResponses"];
    let answer = resp["ANSWER"].as_str().unwrap_or("");

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let has_verify_lambda = pool
        .lambda_config
        .contains_key("VerifyAuthChallengeResponse");
    if let Some(arn) = pool.lambda_config.get("VerifyAuthChallengeResponse") {
        invoke_trigger(
            ctx,
            "VerifyAuthChallengeResponse_Authentication",
            arn,
            &json!({
                "userPoolId": pool_id,
                "userName": session.username,
                "callerContext": { "clientId": client_id },
                "request": { "challengeAnswer": answer }
            }),
        );
    }
    match pool.custom_auth_expected_answer.as_deref() {
        // A fixture pins the expected answer: compare directly.
        Some(expected) => {
            if expected != answer {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect username or password.",
                ));
            }
        }
        // No fixture: only a configured VerifyAuthChallengeResponse Lambda
        // could decide correctness. Without one, fail closed; with one,
        // accept a non-empty answer (synchronous Lambda evaluation is not
        // yet modelled).
        None => {
            if !has_verify_lambda || answer.is_empty() {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect username or password.",
                ));
            }
        }
    }

    let user = pool.users.get(&session.username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let pairs = group_role_pairs(&pool, &user.groups);
    let validity = pool
        .clients
        .get(client_id)
        .map(TokenValidity::from_client)
        .unwrap_or_else(TokenValidity::defaults);
    let result = build_auth_result_validity(
        &user.sub,
        &user.username,
        region,
        pool_id,
        client_id,
        &user.attributes,
        &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
        &pairs,
        &validity,
    );
    drop(pool);
    state.mfa_sessions.remove(session_id);
    info!(username = %session.username, "Cognito: CUSTOM_CHALLENGE success");
    Ok(result)
}

/// Lambda-driven `RespondToAuthChallenge(CUSTOM_CHALLENGE)`:
/// VerifyAuthChallengeResponse judges the answer, then DefineAuthChallenge
/// decides whether to issue tokens, fail, or start another round.
fn verify_custom_auth_lambda(
    state: &CognitoState,
    pool_id: &str,
    client_id: &str,
    session_id: &str,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let region = &ctx.region;
    let account = &ctx.account_id;
    let mut session = state
        .custom_auth_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| {
            AwsError::bad_request("NotAuthorizedException", "Invalid session for the user.")
        })?;
    if !session_still_valid(session.issued_at) {
        state.custom_auth_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user, session is expired.",
        ));
    }
    if session.pool_id != pool_id {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Invalid session for the user.",
        ));
    }

    let invoker = state.lambda_invoker.get().cloned().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidLambdaResponseException",
            "Custom auth requires a configured Lambda invoker",
        )
    })?;
    let invoker = invoker.as_ref();

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
    })?;
    let user = pool
        .users
        .get(&session.username)
        .ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?
        .clone();

    let metadata = client_metadata(input);
    // Empty answers are forwarded to the Lambda (Cognito does not pre-reject
    // them); the trigger decides correctness.
    let answer = input["ChallengeResponses"]["ANSWER"].as_str().unwrap_or("");

    // VerifyAuthChallengeResponse.
    let answer_correct = match pool.lambda_config.get("VerifyAuthChallengeResponse") {
        Some(arn) => {
            let mut extra = json!({
                "challengeAnswer": answer,
                "privateChallengeParameters": session.private_params,
                "session": custom_auth_session_list(&session.rounds),
                "clientMetadata": metadata,
            });
            if let Some(obj) = extra.as_object_mut() {
                obj.insert("userAttributes".to_string(), user_attributes_object(&user));
            }
            let event = custom_auth_event(
                "VerifyAuthChallengeResponse_Authentication",
                region,
                pool_id,
                &session.username,
                client_id,
                &user,
                extra,
            );
            let resp = invoke_custom_auth_trigger(
                invoker,
                "VerifyAuthChallengeResponse",
                arn,
                account,
                region,
                &event,
            )?;
            resp["response"]["answerCorrect"].as_bool().unwrap_or(false)
        }
        // No verify trigger: the answer cannot be judged correct.
        None => false,
    };

    // Record the result on the pending round.
    if let Some(last) = session.rounds.last_mut() {
        last.challenge_result = Some(answer_correct);
    }

    // DefineAuthChallenge with the full history.
    let Some(define_arn) = pool.lambda_config.get("DefineAuthChallenge") else {
        state.custom_auth_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "InvalidLambdaResponseException",
            "DefineAuthChallenge Lambda not found",
        ));
    };
    let define_event = custom_auth_event(
        "DefineAuthChallenge_Authentication",
        region,
        pool_id,
        &session.username,
        client_id,
        &user,
        json!({ "session": custom_auth_session_list(&session.rounds) }),
    );
    let define = invoke_custom_auth_trigger(
        invoker,
        "DefineAuthChallenge",
        define_arn,
        account,
        region,
        &define_event,
    )?;
    let response = &define["response"];

    if response["failAuthentication"].as_bool() == Some(true) {
        state.custom_auth_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Incorrect username or password.",
        ));
    }
    // issueTokens is honoured before the attempt cap, so a success on the
    // final permitted round still wins.
    if response["issueTokens"].as_bool() == Some(true) {
        let result = build_custom_auth_tokens(&pool, region, pool_id, client_id, &user)?;
        drop(pool);
        state.custom_auth_sessions.remove(session_id);
        info!(username = %session.username, "Cognito: CUSTOM_AUTH (lambda) success");
        return Ok(result);
    }
    if session.rounds.len() >= MAX_CUSTOM_AUTH_ROUNDS {
        state.custom_auth_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Max authentication attempts exceeded",
        ));
    }
    if response["challengeName"].as_str() != Some("CUSTOM_CHALLENGE") {
        state.custom_auth_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "InvalidLambdaResponseException",
            "DefineAuthChallenge response invalid",
        ));
    }

    // Next round: CreateAuthChallenge, append the round, return the same
    // Session token.
    let (public, private, metadata_out) = run_create_auth_challenge(
        invoker,
        &pool,
        account,
        region,
        pool_id,
        &session.username,
        client_id,
        &user,
        &session.rounds,
        &metadata,
    )?;
    drop(pool);

    session.rounds.push(CustomAuthRound {
        challenge_metadata: metadata_out,
        challenge_result: None,
    });
    session.private_params = private;
    let username = session.username.clone();
    state
        .custom_auth_sessions
        .insert(session_id.to_string(), session);

    let mut params_json = public;
    params_json
        .entry("USERNAME".to_string())
        .or_insert_with(|| Value::String(username.clone()));
    info!(username = %username, "Cognito: CUSTOM_AUTH (lambda) -> next CUSTOM_CHALLENGE");
    Ok(json!({
        "ChallengeName": "CUSTOM_CHALLENGE",
        "Session": session_id,
        "ChallengeParameters": params_json,
    }))
}

#[cfg(test)]
mod session_expiry_tests {
    use super::*;

    #[test]
    fn fresh_session_is_valid() {
        assert!(session_still_valid(now_epoch()));
    }

    #[test]
    fn session_within_window_is_valid() {
        // 4 minutes old, well inside the 5-minute cap.
        assert!(session_still_valid(now_epoch() - 4 * 60));
    }

    #[test]
    fn session_past_window_is_expired() {
        // 6 minutes old.
        assert!(!session_still_valid(now_epoch() - 6 * 60));
    }
}

#[cfg(test)]
mod auth_flow_tests {
    use super::*;
    use crate::operations::{pools, users};

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    /// Create a pool + client + confirmed user. `explicit_flows` populates the
    /// client's ExplicitAuthFlows; `mfa` sets the pool's MfaConfiguration.
    /// Returns (state, pool_id, client_id).
    fn setup(
        explicit_flows: &[&str],
        mfa: &str,
        username: &str,
        password: &str,
    ) -> (CognitoState, String, String) {
        let state = CognitoState::default();
        let c = ctx();

        let pool = pools::create_user_pool(
            &state,
            &json!({ "PoolName": "p", "MfaConfiguration": mfa }),
            &c,
        )
        .unwrap();
        let pool_id = pool["UserPool"]["Id"].as_str().unwrap().to_string();

        let flows: Vec<Value> = explicit_flows.iter().map(|f| json!(f)).collect();
        let client = pools::create_user_pool_client(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientName": "c",
                "ExplicitAuthFlows": flows,
            }),
            &c,
        )
        .unwrap();
        let client_id = client["UserPoolClient"]["ClientId"]
            .as_str()
            .unwrap()
            .to_string();

        users::admin_create_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": username, "MessageAction": "SUPPRESS" }),
            &c,
        )
        .unwrap();
        users::admin_set_user_password(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": username,
                "Password": password,
                "Permanent": true,
            }),
            &c,
        )
        .unwrap();

        (state, pool_id, client_id)
    }

    /// Force a user's MFA fields directly (no public op covers totp_verified).
    fn set_user_mfa(
        state: &CognitoState,
        pool_id: &str,
        username: &str,
        preferred: Option<&str>,
        totp_verified: bool,
        phone: Option<&str>,
    ) {
        let mut pool = state.user_pools.get_mut(pool_id).unwrap();
        let user = pool.users.get_mut(username).unwrap();
        user.mfa_enabled = true;
        user.mfa_preferred = preferred.map(String::from);
        user.totp_verified = totp_verified;
        if totp_verified {
            user.totp_secret = Some("JBSWY3DPEHPK3PXP".to_string());
        }
        if let Some(p) = phone {
            user.attributes
                .insert("phone_number".to_string(), p.to_string());
        }
    }

    #[test]
    fn initiate_auth_rejects_flow_absent_from_explicit_flows() {
        let (state, _pool, client_id) =
            setup(&["ALLOW_USER_SRP_AUTH"], "OFF", "alice", "Passw0rd!");
        let err = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "alice", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn empty_explicit_flows_denies_password_but_allows_srp() {
        let (state, _pool, client_id) = setup(&[], "OFF", "wes", "Passw0rd!");
        // USER_PASSWORD_AUTH is not in the default flow set.
        let err = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "USER_PASSWORD_AUTH",
                     "AuthParameters": { "USERNAME": "wes", "PASSWORD": "Passw0rd!" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        // USER_SRP_AUTH is part of the default set and gets past the gate to
        // the SRP challenge (which needs SRP_A, absent here, so it fails for a
        // different reason).
        let err = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "USER_SRP_AUTH",
                     "AuthParameters": { "USERNAME": "wes" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_ne!(
            err.message,
            "Auth flow not enabled for this client: USER_SRP_AUTH"
        );
    }

    #[test]
    fn initiate_auth_accepts_listed_flow() {
        let (state, _pool, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "OFF", "bob", "Passw0rd!");
        let res = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "bob", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn admin_initiate_auth_rejects_flow_absent_from_explicit_flows() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_SRP_AUTH"], "OFF", "carol", "Passw0rd!");
        let err = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "carol", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn admin_initiate_auth_accepts_admin_user_password_flow() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_ADMIN_USER_PASSWORD_AUTH"],
            "OFF",
            "dave",
            "Passw0rd!",
        );
        let res = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "dave", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn admin_initiate_auth_accepts_admin_no_srp_alias() {
        let (state, pool_id, client_id) = setup(&["ADMIN_NO_SRP_AUTH"], "OFF", "nora", "Passw0rd!");
        let res = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "ADMIN_NO_SRP_AUTH",
                "AuthParameters": { "USERNAME": "nora", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn admin_initiate_auth_rejects_plain_user_password_auth() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_ADMIN_USER_PASSWORD_AUTH", "ALLOW_USER_PASSWORD_AUTH"],
            "OFF",
            "olga",
            "Passw0rd!",
        );
        let err = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "olga", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn admin_initiate_auth_refresh_token_reissues_tokens() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_ADMIN_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"],
            "OFF",
            "pat",
            "Passw0rd!",
        );
        let first = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "pat", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        let refresh = first["AuthenticationResult"]["RefreshToken"]
            .as_str()
            .unwrap()
            .to_string();
        let res = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": refresh }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
        // AWS does not reissue a refresh token on this flow.
        assert!(res["AuthenticationResult"]["RefreshToken"].is_null());
    }

    #[test]
    fn refresh_token_auth_rejects_garbage_token() {
        let (state, _pool, client_id) = setup(
            &["ALLOW_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"],
            "OFF",
            "quinn",
            "Passw0rd!",
        );
        let err = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": "refresh-nobody.0.deadbeef" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
        assert_eq!(err.message, "Invalid Refresh Token.");
    }

    #[test]
    fn sms_mfa_issuance_and_response() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "eve", "Passw0rd!");
        set_user_mfa(
            &state,
            &pool_id,
            "eve",
            Some("SMS_MFA"),
            false,
            Some("+12345550100"),
        );

        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "eve", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "SMS_MFA");
        assert_eq!(challenge["CodeDeliveryDetails"]["DeliveryMedium"], "SMS");
        let session = challenge["Session"].as_str().unwrap().to_string();

        // Pull the stashed code so the response can pass.
        let code = {
            let pool = state.user_pools.get(&pool_id).unwrap();
            pool.users
                .get("eve")
                .unwrap()
                .pending_verifications
                .get(SMS_MFA_KEY)
                .unwrap()
                .clone()
        };

        let res = respond_to_auth_challenge(
            &state,
            &json!({
                "ClientId": client_id,
                "ChallengeName": "SMS_MFA",
                "Session": session,
                "ChallengeResponses": { "USERNAME": "eve", "SMS_MFA_CODE": code }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn sms_mfa_response_rejects_wrong_code() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "frank", "Passw0rd!");
        set_user_mfa(
            &state,
            &pool_id,
            "frank",
            Some("SMS_MFA"),
            false,
            Some("+12345550100"),
        );
        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "frank", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        let session = challenge["Session"].as_str().unwrap().to_string();
        let err = respond_to_auth_challenge(
            &state,
            &json!({
                "ClientId": client_id,
                "ChallengeName": "SMS_MFA",
                "Session": session,
                "ChallengeResponses": { "USERNAME": "frank", "SMS_MFA_CODE": "000000" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "CodeMismatchException");
    }

    #[test]
    fn email_otp_issuance_and_response() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "grace", "Passw0rd!");
        {
            let mut pool = state.user_pools.get_mut(&pool_id).unwrap();
            let user = pool.users.get_mut("grace").unwrap();
            user.mfa_enabled = true;
            user.mfa_preferred = Some("EMAIL_OTP".to_string());
            user.attributes
                .insert("email".to_string(), "grace@example.com".to_string());
        }

        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "grace", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "EMAIL_OTP");
        let session = challenge["Session"].as_str().unwrap().to_string();
        let code = {
            let pool = state.user_pools.get(&pool_id).unwrap();
            pool.users
                .get("grace")
                .unwrap()
                .pending_verifications
                .get(EMAIL_OTP_KEY)
                .unwrap()
                .clone()
        };

        let res = respond_to_auth_challenge(
            &state,
            &json!({
                "ClientId": client_id,
                "ChallengeName": "EMAIL_OTP",
                "Session": session,
                "ChallengeResponses": { "USERNAME": "grace", "EMAIL_OTP_CODE": code }
            }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn select_mfa_type_when_multiple_factors_then_choose_software() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "heidi", "Passw0rd!");
        // Both software-token and SMS enabled, no preference => SELECT_MFA_TYPE.
        set_user_mfa(&state, &pool_id, "heidi", None, true, Some("+12345550100"));

        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "heidi", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "SELECT_MFA_TYPE");
        let session = challenge["Session"].as_str().unwrap().to_string();

        // Choosing SOFTWARE_TOKEN_MFA re-issues that challenge on the same session.
        let next = respond_to_auth_challenge(
            &state,
            &json!({
                "ClientId": client_id,
                "ChallengeName": "SELECT_MFA_TYPE",
                "Session": session,
                "ChallengeResponses": { "USERNAME": "heidi", "ANSWER": "SOFTWARE_TOKEN_MFA" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(next["ChallengeName"], "SOFTWARE_TOKEN_MFA");
        assert!(next["Session"].is_string());
    }

    #[test]
    fn select_mfa_type_choosing_sms_issues_code() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "ivan", "Passw0rd!");
        set_user_mfa(&state, &pool_id, "ivan", None, true, Some("+12345550100"));

        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "ivan", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "SELECT_MFA_TYPE");
        let session = challenge["Session"].as_str().unwrap().to_string();

        let next = respond_to_auth_challenge(
            &state,
            &json!({
                "ClientId": client_id,
                "ChallengeName": "SELECT_MFA_TYPE",
                "Session": session,
                "ChallengeResponses": { "USERNAME": "ivan", "ANSWER": "SMS_MFA" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(next["ChallengeName"], "SMS_MFA");
        // A code is now stashed for the SMS challenge.
        let pool = state.user_pools.get(&pool_id).unwrap();
        assert!(
            pool.users
                .get("ivan")
                .unwrap()
                .pending_verifications
                .contains_key(SMS_MFA_KEY)
        );
    }

    #[test]
    fn software_token_remains_default_when_only_factor() {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "judy", "Passw0rd!");
        set_user_mfa(&state, &pool_id, "judy", None, true, None);
        let challenge = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "judy", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "SOFTWARE_TOKEN_MFA");
    }

    #[test]
    fn list_users_cursor_round_trip() {
        let (state, pool_id, _client_id) = setup(&[], "OFF", "u-a", "Passw0rd!");
        let c = ctx();
        for name in ["u-b", "u-c", "u-d", "u-e"] {
            users::admin_create_user(
                &state,
                &json!({ "UserPoolId": pool_id, "Username": name, "MessageAction": "SUPPRESS" }),
                &c,
            )
            .unwrap();
        }

        let p1 =
            users::list_users(&state, &json!({ "UserPoolId": pool_id, "Limit": 2 }), &c).unwrap();
        let page1: Vec<String> = p1["Users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["Username"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(page1, vec!["u-a", "u-b"]);
        let token = p1["PaginationToken"].as_str().expect("page1 token");

        let p2 = users::list_users(
            &state,
            &json!({ "UserPoolId": pool_id, "Limit": 2, "PaginationToken": token }),
            &c,
        )
        .unwrap();
        let page2: Vec<String> = p2["Users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["Username"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(page2, vec!["u-c", "u-d"]);
        let token2 = p2["PaginationToken"].as_str().expect("page2 token");

        let p3 = users::list_users(
            &state,
            &json!({ "UserPoolId": pool_id, "Limit": 2, "PaginationToken": token2 }),
            &c,
        )
        .unwrap();
        let page3: Vec<String> = p3["Users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["Username"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(page3, vec!["u-e"]);
        assert!(p3.get("PaginationToken").is_none());
    }

    /// Drive AdminInitiateAuth to a SOFTWARE_TOKEN_MFA challenge and verify the
    /// admin response path actually checks the TOTP code: a wrong code is
    /// rejected and only the correct code mints tokens. Guards against the
    /// admin flow issuing tokens on session validity alone.
    #[test]
    fn admin_software_token_mfa_requires_valid_totp_code() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_ADMIN_USER_PASSWORD_AUTH"],
            "ON",
            "tina",
            "Passw0rd!",
        );
        set_user_mfa(&state, &pool_id, "tina", None, true, None);
        let c = ctx();

        let challenge = admin_initiate_auth(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "AuthFlow": "ADMIN_USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "tina", "PASSWORD": "Passw0rd!" }
            }),
            &c,
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "SOFTWARE_TOKEN_MFA");
        let session = challenge["Session"].as_str().unwrap().to_string();

        let err = admin_respond_to_auth_challenge(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "ChallengeName": "SOFTWARE_TOKEN_MFA",
                "Session": session,
                "ChallengeResponses": { "SOFTWARE_TOKEN_MFA_CODE": "000000" }
            }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "CodeMismatchException");

        let secret = awsim_core::totp::decode_base32("JBSWY3DPEHPK3PXP").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = format!("{:06}", awsim_core::totp::code_at(&secret, now));
        let ok = admin_respond_to_auth_challenge(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "ChallengeName": "SOFTWARE_TOKEN_MFA",
                "Session": session,
                "ChallengeResponses": { "SOFTWARE_TOKEN_MFA_CODE": code }
            }),
            &c,
        )
        .unwrap();
        assert!(
            ok["AuthenticationResult"]["AccessToken"].as_str().is_some(),
            "a valid TOTP code should mint tokens"
        );
    }

    /// An unknown or expired session on AdminRespondToAuthChallenge must fail
    /// with NotAuthorizedException, not return an empty 200 AuthenticationResult.
    #[test]
    fn admin_respond_unknown_session_is_not_authorized() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_ADMIN_USER_PASSWORD_AUTH"],
            "ON",
            "vic",
            "Passw0rd!",
        );
        set_user_mfa(&state, &pool_id, "vic", None, true, None);
        let err = admin_respond_to_auth_challenge(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "ClientId": client_id,
                "ChallengeName": "SOFTWARE_TOKEN_MFA",
                "Session": "does-not-exist",
                "ChallengeResponses": { "SOFTWARE_TOKEN_MFA_CODE": "000000" }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    /// RevokeToken must make a later REFRESH_TOKEN_AUTH fail: the refresh path
    /// has to consult the revoked-token set.
    #[test]
    fn revoked_refresh_token_cannot_mint_tokens() {
        let (state, _pool, client_id) = setup(
            &["ALLOW_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"],
            "OFF",
            "rita",
            "Passw0rd!",
        );
        let c = ctx();
        let auth = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "rita", "PASSWORD": "Passw0rd!" }
            }),
            &c,
        )
        .unwrap();
        let refresh = auth["AuthenticationResult"]["RefreshToken"]
            .as_str()
            .unwrap()
            .to_string();

        let refreshed = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": refresh }
            }),
            &c,
        )
        .unwrap();
        assert!(
            refreshed["AuthenticationResult"]["AccessToken"]
                .as_str()
                .is_some()
        );

        users::revoke_token(
            &state,
            &json!({ "Token": refresh, "ClientId": client_id }),
            &c,
        )
        .unwrap();

        let err = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": refresh }
            }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    /// GlobalSignOut must invalidate refresh tokens issued before it while a
    /// fresh sign-in still mints a usable one.
    #[test]
    fn global_sign_out_invalidates_outstanding_refresh_tokens() {
        let (state, pool_id, client_id) = setup(
            &["ALLOW_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"],
            "OFF",
            "gabe",
            "Passw0rd!",
        );
        let c = ctx();
        let auth = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "gabe", "PASSWORD": "Passw0rd!" }
            }),
            &c,
        )
        .unwrap();
        let access = auth["AuthenticationResult"]["AccessToken"]
            .as_str()
            .unwrap()
            .to_string();

        // A refresh token stamped before the sign-out instant.
        let sub = state
            .user_pools
            .get(&pool_id)
            .unwrap()
            .users
            .get("gabe")
            .unwrap()
            .sub
            .clone();
        let old_refresh = format!("refresh-{sub}.0.{}", Uuid::new_v4());

        users::global_sign_out(&state, &json!({ "AccessToken": access }), &c).unwrap();

        let err = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": old_refresh }
            }),
            &c,
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");

        let reauth = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "gabe", "PASSWORD": "Passw0rd!" }
            }),
            &c,
        )
        .unwrap();
        let fresh_refresh = reauth["AuthenticationResult"]["RefreshToken"]
            .as_str()
            .unwrap()
            .to_string();
        let ok = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "AuthParameters": { "REFRESH_TOKEN": fresh_refresh }
            }),
            &c,
        )
        .unwrap();
        assert!(ok["AuthenticationResult"]["AccessToken"].as_str().is_some());
    }

    /// A pool with MfaConfiguration ON and a user with no configured factor must
    /// return an MFA_SETUP challenge, never tokens.
    #[test]
    fn mfa_on_without_factor_challenges_setup() {
        let (state, _pool, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "mona", "Passw0rd!");
        let res = initiate_auth(
            &state,
            &json!({
                "ClientId": client_id,
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": { "USERNAME": "mona", "PASSWORD": "Passw0rd!" }
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(res["ChallengeName"], "MFA_SETUP");
        assert!(res.get("AuthenticationResult").is_none());
        assert!(res["Session"].as_str().is_some());
    }

    /// AdminSetUserPassword without Permanent defaults to a temporary password,
    /// placing the user in FORCE_CHANGE_PASSWORD.
    #[test]
    fn admin_set_user_password_defaults_to_temporary() {
        let (state, pool_id, _client_id) = setup(&[], "OFF", "perry", "Passw0rd!");
        users::admin_set_user_password(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "perry", "Password": "NewPassw0rd!" }),
            &ctx(),
        )
        .unwrap();
        let status = state
            .user_pools
            .get(&pool_id)
            .unwrap()
            .users
            .get("perry")
            .unwrap()
            .status
            .clone();
        assert_eq!(status, "FORCE_CHANGE_PASSWORD");
    }

    /// Put a freshly created user into FORCE_CHANGE_PASSWORD (the state
    /// AdminCreateUser leaves them in) and return the issued challenge session.
    fn force_change_setup() -> (CognitoState, String, String, String) {
        let c = ctx();
        let state = CognitoState::default();
        let pool = pools::create_user_pool(&state, &json!({ "PoolName": "p" }), &c).unwrap();
        let pool_id = pool["UserPool"]["Id"].as_str().unwrap().to_string();
        let client = pools::create_user_pool_client(
            &state,
            &json!({ "UserPoolId": pool_id, "ClientName": "c",
                     "ExplicitAuthFlows": ["ALLOW_USER_PASSWORD_AUTH"] }),
            &c,
        )
        .unwrap();
        let client_id = client["UserPoolClient"]["ClientId"]
            .as_str()
            .unwrap()
            .to_string();
        users::admin_create_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "dan",
                     "TemporaryPassword": "Temp@1234", "MessageAction": "SUPPRESS" }),
            &c,
        )
        .unwrap();
        let res = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "USER_PASSWORD_AUTH",
                     "AuthParameters": { "USERNAME": "dan", "PASSWORD": "Temp@1234" } }),
            &c,
        )
        .unwrap();
        assert_eq!(res["ChallengeName"], "NEW_PASSWORD_REQUIRED");
        // userAttributes is a JSON object string, not an array of Name/Value.
        let attrs = res["ChallengeParameters"]["userAttributes"]
            .as_str()
            .unwrap();
        assert!(serde_json::from_str::<serde_json::Map<String, Value>>(attrs).is_ok());
        let session = res["Session"].as_str().unwrap().to_string();
        (state, pool_id, client_id, session)
    }

    #[test]
    fn new_password_required_completes_with_valid_session() {
        let (state, _pool, client_id, session) = force_change_setup();
        let res = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "NEW_PASSWORD_REQUIRED",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "dan", "NEW_PASSWORD": "Brandnew1!" } }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn new_password_required_rejects_forged_session() {
        let (state, _pool, client_id, _session) = force_change_setup();
        let err = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "NEW_PASSWORD_REQUIRED",
                     "Session": "made-up-session",
                     "ChallengeResponses": { "USERNAME": "dan", "NEW_PASSWORD": "Brandnew1!" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
        // The user must remain unable to authenticate with the attacker password.
        let still_forced = state
            .user_pools
            .get(&_pool)
            .unwrap()
            .users
            .get("dan")
            .unwrap()
            .status
            .clone();
        assert_eq!(still_forced, "FORCE_CHANGE_PASSWORD");
    }

    #[test]
    fn mfa_setup_challenge_completes_after_software_token_verified() {
        use crate::operations::mfa;
        // Pool with MFA ON; a fresh user has no factor, so InitiateAuth issues
        // an MFA_SETUP challenge instead of tokens.
        let (state, _pool, client_id) =
            setup(&["ALLOW_USER_PASSWORD_AUTH"], "ON", "tina", "Passw0rd!");
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "USER_PASSWORD_AUTH",
                     "AuthParameters": { "USERNAME": "tina", "PASSWORD": "Passw0rd!" } }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "MFA_SETUP");
        let session = challenge["Session"].as_str().unwrap().to_string();

        // Associate + verify a software token using the same session.
        let assoc =
            mfa::associate_software_token(&state, &json!({ "Session": session }), &ctx()).unwrap();
        let secret = assoc["SecretCode"].as_str().unwrap();
        let bytes = awsim_core::totp::decode_base32(secret).unwrap();
        let code = format!("{:06}", awsim_core::totp::code_at(&bytes, now_epoch()));
        mfa::verify_software_token(
            &state,
            &json!({ "Session": session, "UserCode": code }),
            &ctx(),
        )
        .unwrap();

        // Completing the MFA_SETUP challenge now issues tokens instead of
        // echoing the challenge forever.
        let res = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "MFA_SETUP",
                     "Session": session, "ChallengeResponses": {} }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn custom_auth_fails_closed_without_fixture_or_lambda() {
        let (state, _pool, client_id) = setup(&["ALLOW_CUSTOM_AUTH"], "OFF", "rex", "Passw0rd!");
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "CUSTOM_AUTH",
                     "AuthParameters": { "USERNAME": "rex" } }),
            &ctx(),
        )
        .unwrap();
        let session = challenge["Session"].as_str().unwrap().to_string();
        // No DefineAuthChallenge lambda and no expected-answer fixture: any
        // answer must be rejected, not accepted.
        let err = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "rex", "ANSWER": "anything" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn custom_auth_succeeds_with_expected_answer_fixture() {
        let (state, pool_id, client_id) = setup(&["ALLOW_CUSTOM_AUTH"], "OFF", "sam", "Passw0rd!");
        state
            .user_pools
            .get_mut(&pool_id)
            .unwrap()
            .custom_auth_expected_answer = Some("1337".to_string());
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "CUSTOM_AUTH",
                     "AuthParameters": { "USERNAME": "sam" } }),
            &ctx(),
        )
        .unwrap();
        let session = challenge["Session"].as_str().unwrap().to_string();
        let res = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "sam", "ANSWER": "1337" } }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    /// Mock invoker implementing an OTP-style custom-auth flow: the answer
    /// "1234" is correct; up to three rounds before failing.
    struct CustomAuthMock;
    impl awsim_core::LambdaInvoker for CustomAuthMock {
        fn invoke(
            &self,
            _function: &str,
            payload: &Value,
            _account: &str,
            _region: &str,
        ) -> Result<Value, AwsError> {
            let src = payload["triggerSource"].as_str().unwrap_or("");
            let req = &payload["request"];
            let resp = if src.starts_with("DefineAuthChallenge") {
                let session = req["session"].as_array().cloned().unwrap_or_default();
                let last_correct =
                    session.last().and_then(|s| s["challengeResult"].as_bool()) == Some(true);
                if last_correct {
                    json!({ "issueTokens": true })
                } else if session.len() >= 3 {
                    json!({ "failAuthentication": true })
                } else {
                    json!({ "challengeName": "CUSTOM_CHALLENGE" })
                }
            } else if src.starts_with("CreateAuthChallenge") {
                json!({
                    "publicChallengeParameters": { "type": "otp" },
                    "privateChallengeParameters": { "answer": "1234" },
                    "challengeMetadata": "otp-round"
                })
            } else {
                json!({ "answerCorrect": req["challengeAnswer"].as_str() == Some("1234") })
            };
            Ok(json!({ "response": resp }))
        }
    }

    /// Pool + client + user wired for the lambda-driven CUSTOM_AUTH path.
    fn lambda_custom_auth_setup(username: &str) -> (CognitoState, String, String) {
        let (state, pool_id, client_id) =
            setup(&["ALLOW_CUSTOM_AUTH"], "OFF", username, "Passw0rd!");
        {
            let mut pool = state.user_pools.get_mut(&pool_id).unwrap();
            for trigger in [
                "DefineAuthChallenge",
                "CreateAuthChallenge",
                "VerifyAuthChallengeResponse",
            ] {
                pool.lambda_config
                    .insert(trigger.to_string(), format!("arn:fn:{trigger}"));
            }
        }
        state
            .lambda_invoker
            .set(std::sync::Arc::new(CustomAuthMock));
        (state, pool_id, client_id)
    }

    #[test]
    fn lambda_custom_auth_succeeds_on_correct_answer() {
        let (state, _pool, client_id) = lambda_custom_auth_setup("lex");
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "CUSTOM_AUTH",
                     "AuthParameters": { "USERNAME": "lex" } }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(challenge["ChallengeName"], "CUSTOM_CHALLENGE");
        // The public challenge parameter from CreateAuthChallenge is surfaced.
        assert_eq!(challenge["ChallengeParameters"]["type"], "otp");
        let session = challenge["Session"].as_str().unwrap().to_string();
        let res = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "lex", "ANSWER": "1234" } }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn lambda_custom_auth_retries_then_succeeds() {
        let (state, _pool, client_id) = lambda_custom_auth_setup("max");
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "CUSTOM_AUTH",
                     "AuthParameters": { "USERNAME": "max" } }),
            &ctx(),
        )
        .unwrap();
        let session = challenge["Session"].as_str().unwrap().to_string();
        // Wrong answer -> another CUSTOM_CHALLENGE on the same session.
        let again = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "max", "ANSWER": "nope" } }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(again["ChallengeName"], "CUSTOM_CHALLENGE");
        assert_eq!(again["Session"], session);
        // Correct answer on the retry -> tokens.
        let res = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "max", "ANSWER": "1234" } }),
            &ctx(),
        )
        .unwrap();
        assert!(res["AuthenticationResult"]["AccessToken"].is_string());
    }

    #[test]
    fn lambda_custom_auth_fails_after_max_attempts() {
        let (state, _pool, client_id) = lambda_custom_auth_setup("rey");
        let challenge = initiate_auth(
            &state,
            &json!({ "ClientId": client_id, "AuthFlow": "CUSTOM_AUTH",
                     "AuthParameters": { "USERNAME": "rey" } }),
            &ctx(),
        )
        .unwrap();
        let session = challenge["Session"].as_str().unwrap().to_string();
        // Two wrong answers keep the flow going.
        for _ in 0..2 {
            let r = respond_to_auth_challenge(
                &state,
                &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                         "Session": session,
                         "ChallengeResponses": { "USERNAME": "rey", "ANSWER": "nope" } }),
                &ctx(),
            )
            .unwrap();
            assert_eq!(r["ChallengeName"], "CUSTOM_CHALLENGE");
        }
        // Third wrong answer hits the attempt cap / failAuthentication.
        let err = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "CUSTOM_CHALLENGE",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "rey", "ANSWER": "nope" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn new_password_required_rejects_username_session_mismatch() {
        let (state, pool_id, client_id, session) = force_change_setup();
        users::admin_create_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "erin",
                     "TemporaryPassword": "Temp@1234", "MessageAction": "SUPPRESS" }),
            &ctx(),
        )
        .unwrap();
        // dan's session must not let a caller rewrite erin's password.
        let err = respond_to_auth_challenge(
            &state,
            &json!({ "ClientId": client_id, "ChallengeName": "NEW_PASSWORD_REQUIRED",
                     "Session": session,
                     "ChallengeResponses": { "USERNAME": "erin", "NEW_PASSWORD": "Brandnew1!" } }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }
}
