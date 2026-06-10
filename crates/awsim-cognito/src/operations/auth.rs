use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::jwt::{self, GroupRolePair};
use crate::state::{CognitoState, CognitoUser, MfaSession, UserPool, UserPoolClient};

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
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid session"))?;
    if !session_still_valid(session_meta.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "MFA session has expired; restart the auth flow",
        ));
    }

    let pool = state
        .user_pools
        .get(&session_meta.pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "Pool not found"))?;
    let user = pool
        .users
        .get(&session_meta.username)
        .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
    let expected = user
        .pending_verifications
        .get(stash_key)
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "No MFA code was issued"))?;
    if expected.as_str() != user_code {
        return Err(AwsError::bad_request(
            "CodeMismatchException",
            "Invalid MFA code",
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
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid session"))?;
    if !session_still_valid(session_meta.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "MFA session has expired; restart the auth flow",
        ));
    }

    let pool = state
        .user_pools
        .get(&session_meta.pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "Pool not found"))?;
    let user = pool
        .users
        .get(&session_meta.username)
        .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
    let secret = user.totp_secret.as_deref().ok_or_else(|| {
        AwsError::forbidden(
            "NotAuthorizedException",
            "User has no software token configured",
        )
    })?;
    if !awsim_core::totp::verify_str(secret, user_code, 1) {
        return Err(AwsError::bad_request(
            "CodeMismatchException",
            "Invalid software token code",
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
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "Refresh Token has been revoked",
        ));
    }
    if let Some(signed_out_at) = user.signed_out_at
        && jwt::refresh_token_issued_at(refresh_tok).is_none_or(|issued| issued < signed_out_at)
    {
        return Err(AwsError::forbidden(
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
    let access_tok = jwt::access_token(
        user_sub,
        region,
        pool_id,
        client_id,
        username,
        &default_scopes,
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
    if flows.is_empty() {
        return true;
    }
    let has = |name: &str| flows.iter().any(|f| f == name);
    match auth_flow {
        "USER_SRP_AUTH" => has("ALLOW_USER_SRP_AUTH"),
        "USER_PASSWORD_AUTH" => has("ALLOW_USER_PASSWORD_AUTH") || has("USER_PASSWORD_AUTH"),
        // AdminInitiateAuth-only flow; gated by its own ALLOW_ entry or the
        // legacy ADMIN_NO_SRP_AUTH name.
        "ADMIN_USER_PASSWORD_AUTH" => {
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        )
    })?;

    // Reject flows the client's ExplicitAuthFlows excludes, before doing any
    // credential work, mirroring Cognito's up-front validation.
    {
        let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
            AwsError::not_found("ResourceNotFoundException", "User pool not found")
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
        "CUSTOM_AUTH" => start_custom_auth_challenge(state, client_id, &pool_id, params, ctx),
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
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
                })?;
                super::users::resolve_username_for_signin(&pool, raw_username).ok_or_else(|| {
                    AwsError::not_found(
                        "UserNotFoundException",
                        format!("User not found: {raw_username}"),
                    )
                })?
            };
            let username = username.as_str();

            // Pre-Authentication trigger (fire-and-forget) — read pool with
            // an immutable borrow first to fire the trigger, then drop so we
            // can take a mutable borrow for the lockout bookkeeping below.
            {
                let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
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
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
                })?;
                let block_action = super::auth_policy::compromised_credentials_action_for(
                    &pool,
                    Some(client_id),
                    "SIGN_IN",
                );
                let compromised = super::auth_policy::is_compromised_password(password);

                let user = pool.users.get_mut(username).ok_or_else(|| {
                    AwsError::not_found(
                        "UserNotFoundException",
                        format!("User not found: {username}"),
                    )
                })?;
                if !user.enabled {
                    return Err(AwsError::forbidden(
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
                    return Err(AwsError::forbidden(
                        "NotAuthorizedException",
                        "Incorrect username or password",
                    ));
                }

                if compromised && block_action.as_deref() == Some("BLOCK") {
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, true),
                    );
                    return Err(AwsError::forbidden(
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
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
            })?;

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed",
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
                let session_id = Uuid::new_v4().to_string();
                let user_attrs_json = serde_json::to_string(
                    &user
                        .attributes
                        .iter()
                        .map(|(k, v)| json!({"Name":k,"Value":v}))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_default();
                info!(username = %username, "Cognito: InitiateAuth → NEW_PASSWORD_REQUIRED");
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
                        AwsError::not_found("ResourceNotFoundException", "User pool not found")
                    })?;
                    let user = pool.users.get_mut(username).ok_or_else(|| {
                        AwsError::not_found(
                            "UserNotFoundException",
                            format!("User not found: {username}"),
                        )
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
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
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
            let refresh_tok = params["REFRESH_TOKEN"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameterException", "REFRESH_TOKEN is required")
            })?;

            // Extract sub from our opaque refresh token format: "refresh-{sub}-{uuid}"
            let sub = refresh_tok
                .strip_prefix("refresh-")
                .and_then(|s| s.split('.').next())
                .unwrap_or("unknown");
            // Cognito accepts SECRET_HASH on REFRESH_TOKEN_AUTH using the
            // *sub* as the username component (since the original username
            // may not be on the wire). Skip when public-client.
            crate::secret_hash::validate_for_client(
                state,
                client_id,
                params["SECRET_HASH"].as_str(),
                sub,
            )?;

            // Find user by sub
            let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool
                .users
                .values()
                .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
                .ok_or_else(|| {
                    AwsError::not_found("UserNotFoundException", "User not found for refresh token")
                })?;
            ensure_refresh_token_active(state, user, refresh_tok)?;

            let pairs = group_role_pairs(&pool, &user.groups);
            let validity = pool
                .clients
                .get(client_id)
                .map(TokenValidity::from_client)
                .unwrap_or_else(TokenValidity::defaults);
            // include_refresh=false: AWS doesn't reissue a RefreshToken
            // on REFRESH_TOKEN_AUTH; the SPA keeps the original one.
            Ok(build_auth_result_inner(
                &user.sub,
                &user.username,
                &ctx.region,
                &pool_id,
                client_id,
                &user.attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
                false,
            ))
        }
        flow => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unsupported AuthFlow: {flow}"),
        )),
    }
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
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("User pool not found: {pool_id}"),
            )
        })?;

        let client = pool.clients.get(client_id).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("Client not found: {client_id}"),
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
        "USER_PASSWORD_AUTH" | "ADMIN_USER_PASSWORD_AUTH" | "USER_SRP_AUTH" => {
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
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
                })?;
                super::users::resolve_username_for_signin(&pool, raw_username).ok_or_else(|| {
                    AwsError::not_found(
                        "UserNotFoundException",
                        format!("User not found: {raw_username}"),
                    )
                })?
            };
            let username = username.as_str();

            // Pre-Authentication trigger (fire-and-forget) — separate
            // immutable scope so we can take a mutable borrow below.
            {
                let pool = state.user_pools.get(pool_id).ok_or_else(|| {
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
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
                    AwsError::not_found("ResourceNotFoundException", "User pool not found")
                })?;
                let block_action = super::auth_policy::compromised_credentials_action_for(
                    &pool,
                    Some(client_id),
                    "SIGN_IN",
                );
                let compromised = super::auth_policy::is_compromised_password(password);

                let user = pool.users.get_mut(username).ok_or_else(|| {
                    AwsError::not_found(
                        "UserNotFoundException",
                        format!("User not found: {username}"),
                    )
                })?;
                if !user.enabled {
                    return Err(AwsError::forbidden(
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
                    return Err(AwsError::forbidden(
                        "NotAuthorizedException",
                        "Incorrect username or password",
                    ));
                }

                if compromised && block_action.as_deref() == Some("BLOCK") {
                    super::auth_policy::record_auth_event(
                        user,
                        super::auth_policy::build_signin_event(false, true),
                    );
                    return Err(AwsError::forbidden(
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
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
            })?;

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed",
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
                let session_id = Uuid::new_v4().to_string();
                let user_attrs_json = serde_json::to_string(
                    &user
                        .attributes
                        .iter()
                        .map(|(k, v)| json!({"Name":k,"Value":v}))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_default();
                info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth → NEW_PASSWORD_REQUIRED");
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
                        AwsError::not_found("ResourceNotFoundException", "User pool not found")
                    })?;
                    let user = pool.users.get_mut(username).ok_or_else(|| {
                        AwsError::not_found(
                            "UserNotFoundException",
                            format!("User not found: {username}"),
                        )
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
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
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
            let username = responses["USERNAME"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "USERNAME is required in ChallengeResponses",
                )
            })?;
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "NEW_PASSWORD is required in ChallengeResponses",
                )
            })?;

            let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?;
            let policy = pool.policies.clone();
            let user = pool.users.get_mut(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
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
                .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid session"))?;
            if !session_still_valid(session_meta.issued_at) {
                state.mfa_sessions.remove(session_id);
                return Err(AwsError::forbidden(
                    "NotAuthorizedException",
                    "MFA session has expired; restart the auth flow",
                ));
            }

            let challenge = match answer {
                "SMS_MFA" => {
                    let mut pool =
                        state
                            .user_pools
                            .get_mut(&session_meta.pool_id)
                            .ok_or_else(|| {
                                AwsError::not_found("ResourceNotFoundException", "Pool not found")
                            })?;
                    let user = pool.users.get_mut(&session_meta.username).ok_or_else(|| {
                        AwsError::not_found("UserNotFoundException", "User not found")
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
        "MFA_SETUP" => Ok(json!({
            "ChallengeName": "MFA_SETUP",
            "ChallengeParameters": {},
            "Session": input["Session"]
        })),
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
            let username = responses["USERNAME"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "USERNAME is required in ChallengeResponses",
                )
            })?;
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameterException",
                    "NEW_PASSWORD is required in ChallengeResponses",
                )
            })?;

            let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
                AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("User pool not found: {pool_id}"),
                )
            })?;

            if !pool.clients.contains_key(client_id) {
                return Err(AwsError::not_found(
                    "ResourceNotFoundException",
                    format!("Client not found: {client_id}"),
                ));
            }

            let policy = pool.policies.clone();
            let user = pool.users.get_mut(username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
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
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No pool found for client: {client_id}"),
        )
    })?;

    // Extract sub from our opaque refresh token format: "refresh-{sub}.{uuid}"
    let sub = refresh_tok
        .strip_prefix("refresh-")
        .and_then(|s| s.split('.').next())
        .unwrap_or("unknown");

    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Pool not found: {pool_id}"),
        )
    })?;
    let user = pool
        .users
        .values()
        .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
        .ok_or_else(|| {
            AwsError::not_found("UserNotFoundException", "User not found for refresh token")
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
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid access token"))?;

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

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
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

    let pool = state
        .user_pools
        .get(pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let resolved_username = super::users::resolve_username_for_signin(&pool, raw_username)
        .ok_or_else(|| {
            AwsError::not_found(
                "UserNotFoundException",
                format!("User not found: {raw_username}"),
            )
        })?;

    let user = pool
        .users
        .get(&resolved_username)
        .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
    if !user.enabled {
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "User is disabled.",
        ));
    }
    let salt_hex = user.srp_salt.clone().ok_or_else(|| {
        AwsError::forbidden(
            "NotAuthorizedException",
            "User has no SRP material; ask the admin to reset the password",
        )
    })?;
    let verifier_hex = user
        .srp_verifier
        .clone()
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "User has no SRP verifier"))?;
    let verifier_big = BigUint::from_str_radix(&verifier_hex, 16)
        .map_err(|_| AwsError::internal("Stored SRP verifier is not valid hex"))?;

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
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid session"))?;
    if session.client_id != client_id || session.pool_id != pool_id {
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "Session does not match client or pool",
        ));
    }
    if !session_still_valid(session.issued_at) {
        state.srp_sessions.remove(session_id);
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "SRP session has expired; restart the auth flow",
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
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "PASSWORD_CLAIM_SECRET_BLOCK does not match server-issued value",
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

    let pool = state
        .user_pools
        .get(pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let user = pool
        .users
        .get(&session.username)
        .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
    let verifier_hex = user.srp_verifier.clone().ok_or_else(|| {
        AwsError::internal("User has no SRP verifier; cannot complete PASSWORD_VERIFIER")
    })?;
    let verifier_big = BigUint::from_str_radix(&verifier_hex, 16)
        .map_err(|_| AwsError::internal("Stored SRP verifier is not valid hex"))?;
    let b_priv = BigUint::from_str_radix(&session.b_priv_hex, 16)
        .map_err(|_| AwsError::internal("Stored SRP b is not valid hex"))?;
    let b_pub = BigUint::from_str_radix(&session.b_pub_hex, 16)
        .map_err(|_| AwsError::internal("Stored SRP B is not valid hex"))?;

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
            AwsError::forbidden("NotAuthorizedException", "SRP key derivation failed")
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
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "PASSWORD_CLAIM_SIGNATURE does not match",
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

/// Phase 1 of CUSTOM_AUTH. Real Cognito invokes the DefineAuthChallenge and
/// CreateAuthChallenge Lambda triggers to decide what challenge to issue
/// and what parameters to expose to the client. awsim has no synchronous
/// Lambda invocation path, so we publish those triggers as fire-and-forget
/// events and emit a CUSTOM_CHALLENGE backed by the pool's
/// `custom_auth_challenge_parameters` fixture. Tests can configure that
/// fixture or the expected answer directly via UpdateUserPool.
fn start_custom_auth_challenge(
    state: &CognitoState,
    client_id: &str,
    pool_id: &str,
    params: &serde_json::Value,
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

    let pool = state
        .user_pools
        .get(pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
    let resolved_username = super::users::resolve_username_for_signin(&pool, raw_username)
        .ok_or_else(|| {
            AwsError::not_found(
                "UserNotFoundException",
                format!("User not found: {raw_username}"),
            )
        })?;
    if let Some(user) = pool.users.get(&resolved_username)
        && !user.enabled
    {
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "User is disabled.",
        ));
    }

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

/// Phase 2 of CUSTOM_AUTH. Real Cognito would call the
/// VerifyAuthChallengeResponse Lambda to decide if `ANSWER` is correct.
/// awsim does the simplest equivalent: compare against the pool's
/// `custom_auth_expected_answer` fixture (when set), or accept any
/// non-empty answer otherwise. The Lambda trigger is still emitted as a
/// fire-and-forget event so tests can observe it.
fn verify_custom_auth_response(
    state: &CognitoState,
    pool_id: &str,
    client_id: &str,
    region: &str,
    input: &serde_json::Value,
    ctx: &RequestContext,
) -> Result<serde_json::Value, AwsError> {
    let session_id = input["Session"].as_str().unwrap_or("");
    let session = state
        .mfa_sessions
        .get(session_id)
        .map(|e| e.value().clone())
        .ok_or_else(|| AwsError::forbidden("NotAuthorizedException", "Invalid session"))?;
    if !session_still_valid(session.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "Custom auth session has expired; restart the auth flow",
        ));
    }
    if session.pool_id != pool_id {
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "Session does not match pool",
        ));
    }

    let resp = &input["ChallengeResponses"];
    let answer = resp["ANSWER"].as_str().unwrap_or("");
    if answer.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "ChallengeResponses.ANSWER is required",
        ));
    }

    let pool = state
        .user_pools
        .get(pool_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", "User pool not found"))?;
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
    if let Some(expected) = pool.custom_auth_expected_answer.as_deref()
        && expected != answer
    {
        return Err(AwsError::forbidden(
            "NotAuthorizedException",
            "Incorrect answer to custom challenge",
        ));
    }

    let user = pool
        .users
        .get(&session.username)
        .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
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
    fn sms_mfa_issuance_and_response() {
        let (state, pool_id, client_id) = setup(&[], "ON", "eve", "Passw0rd!");
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
        let (state, pool_id, client_id) = setup(&[], "ON", "frank", "Passw0rd!");
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
        let (state, pool_id, client_id) = setup(&[], "ON", "grace", "Passw0rd!");
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
        let (state, pool_id, client_id) = setup(&[], "ON", "heidi", "Passw0rd!");
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
        let (state, pool_id, client_id) = setup(&[], "ON", "ivan", "Passw0rd!");
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
        let (state, pool_id, client_id) = setup(&[], "ON", "judy", "Passw0rd!");
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
}
