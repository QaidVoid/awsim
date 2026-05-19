use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::jwt::{self, GroupRolePair};
use crate::state::{CognitoState, MfaSession, UserPool, UserPoolClient};

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

// ---------------------------------------------------------------------------
// InitiateAuth
// ---------------------------------------------------------------------------

pub fn initiate_auth(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let auth_flow = input["AuthFlow"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AuthFlow is required"))?;
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

    match auth_flow {
        "USER_SRP_AUTH" => start_srp_challenge(state, client_id, &pool_id, params),
        "CUSTOM_AUTH" => start_custom_auth_challenge(state, client_id, &pool_id, params, ctx),
        "USER_PASSWORD_AUTH" => {
            let raw_username = params["USERNAME"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
            let password = params["PASSWORD"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PASSWORD is required"))?;
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
                        "Incorrect username or password",
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

            if mfa_required && user.totp_verified {
                // Return SOFTWARE_TOKEN_MFA challenge
                let session_id = Uuid::new_v4().to_string();
                let _ = user;
                drop(pool);
                state.mfa_sessions.insert(
                    session_id.clone(),
                    MfaSession {
                        pool_id: pool_id.clone(),
                        username: username.to_string(),
                        issued_at: now_epoch(),
                    },
                );
                info!(username = %username, "Cognito: InitiateAuth → MFA challenge");
                return Ok(json!({
                    "ChallengeName": "SOFTWARE_TOKEN_MFA",
                    "Session": session_id,
                    "ChallengeParameters": {
                        "USER_ID_FOR_SRP": username,
                    }
                }));
            }

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
            // Accept any refresh token for local dev; return fresh tokens.
            let refresh_tok = params["REFRESH_TOKEN"].as_str().ok_or_else(|| {
                AwsError::bad_request("InvalidParameter", "REFRESH_TOKEN is required")
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
            "InvalidParameter",
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let auth_flow = input["AuthFlow"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AuthFlow is required"))?;
    let params = &input["AuthParameters"];

    {
        let pool = state.user_pools.get(pool_id).ok_or_else(|| {
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
    }

    match auth_flow {
        "USER_PASSWORD_AUTH" | "ADMIN_USER_PASSWORD_AUTH" | "USER_SRP_AUTH" => {
            let raw_username = params["USERNAME"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
            let password = params["PASSWORD"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PASSWORD is required"))?;
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
                        "Incorrect username or password",
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

            if mfa_required && user.totp_verified {
                let session_id = Uuid::new_v4().to_string();
                let _ = user;
                drop(pool);
                state.mfa_sessions.insert(
                    session_id.clone(),
                    MfaSession {
                        pool_id: pool_id.to_string(),
                        username: username.to_string(),
                        issued_at: now_epoch(),
                    },
                );
                info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth → MFA challenge");
                return Ok(json!({
                    "ChallengeName": "SOFTWARE_TOKEN_MFA",
                    "Session": session_id,
                    "ChallengeParameters": {
                        "USER_ID_FOR_SRP": username,
                    }
                }));
            }

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
            "InvalidParameter",
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
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let challenge_name = input["ChallengeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ChallengeName is required"))?;
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
                    "InvalidParameter",
                    "USERNAME is required in ChallengeResponses",
                )
            })?;
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
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
        "SOFTWARE_TOKEN_MFA" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            let user_code = input["ChallengeResponses"]["SOFTWARE_TOKEN_MFA_CODE"]
                .as_str()
                .ok_or_else(|| {
                    AwsError::bad_request(
                        "InvalidParameterException",
                        "ChallengeResponses.SOFTWARE_TOKEN_MFA_CODE is required",
                    )
                })?;

            // Look up the session without consuming it so that an invalid
            // code surfaces NotAuthorizedException and the user can retry,
            // matching real Cognito's behaviour. Only on a successful
            // verify do we drop the session.
            let session_meta = state
                .mfa_sessions
                .get(session_id)
                .map(|e| e.value().clone())
                .ok_or_else(|| {
                    AwsError::bad_request("NotAuthorizedException", "Invalid session")
                })?;
            if !session_still_valid(session_meta.issued_at) {
                state.mfa_sessions.remove(session_id);
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "MFA session has expired; restart the auth flow",
                ));
            }

            let pool = state.user_pools.get(&session_meta.pool_id).ok_or_else(|| {
                AwsError::not_found("ResourceNotFoundException", "Pool not found")
            })?;
            let user = pool
                .users
                .get(&session_meta.username)
                .ok_or_else(|| AwsError::not_found("UserNotFoundException", "User not found"))?;
            let secret = user.totp_secret.as_deref().ok_or_else(|| {
                AwsError::bad_request(
                    "NotAuthorizedException",
                    "User has no software token configured",
                )
            })?;
            if !crate::totp::verify(secret, user_code) {
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
                &ctx.region,
                &session_meta.pool_id,
                client_id,
                &user.attributes,
                &crate::operations::users::client_read_set(&pool, client_id).unwrap_or_default(),
                &pairs,
                &validity,
            );
            // Drop the consumed session only after the code passed.
            drop(pool);
            state.mfa_sessions.remove(session_id);
            info!(username = %session_meta.username, "Cognito: RespondToAuthChallenge SOFTWARE_TOKEN_MFA success");
            Ok(result)
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
            "InvalidParameter",
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let challenge_name = input["ChallengeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ChallengeName is required"))?;
    let responses = &input["ChallengeResponses"];

    match challenge_name {
        "NEW_PASSWORD_REQUIRED" => {
            let username = responses["USERNAME"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
                    "USERNAME is required in ChallengeResponses",
                )
            })?;
            let new_password = responses["NEW_PASSWORD"].as_str().ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidParameter",
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
        "SOFTWARE_TOKEN_MFA" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            if let Some(session) = state.mfa_sessions.remove(session_id) {
                if !session_still_valid(session.1.issued_at) {
                    return Err(AwsError::bad_request(
                        "NotAuthorizedException",
                        "MFA session has expired; restart the auth flow",
                    ));
                }
                let pool = state.user_pools.get(&session.1.pool_id).ok_or_else(|| {
                    AwsError::not_found("ResourceNotFoundException", "Pool not found")
                })?;
                let user = pool.users.get(&session.1.username).ok_or_else(|| {
                    AwsError::not_found("UserNotFoundException", "User not found")
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
                    &ctx.region,
                    &session.1.pool_id,
                    client_id,
                    &user.attributes,
                    &crate::operations::users::client_read_set(&pool, client_id)
                        .unwrap_or_default(),
                    &pairs,
                    &validity,
                );
                info!(username = %session.1.username, pool_id = %pool_id, "Cognito: AdminRespondToAuthChallenge SOFTWARE_TOKEN_MFA success");
                Ok(result)
            } else {
                Ok(json!({ "AuthenticationResult": {} }))
            }
        }
        name => Err(AwsError::bad_request(
            "InvalidParameter",
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
    let refresh_tok = input["RefreshToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "RefreshToken is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

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
    let token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    let username = crate::jwt::extract_username_from_access_token(token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

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

    let raw_username = params["USERNAME"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        params["SECRET_HASH"].as_str(),
        raw_username,
    )?;
    let srp_a_hex = params["SRP_A"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SRP_A is required"))?;
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
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid session"))?;
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
        return Err(AwsError::bad_request(
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
    let raw_username = params["USERNAME"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
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
        return Err(AwsError::bad_request(
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
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid session"))?;
    if !session_still_valid(session.issued_at) {
        state.mfa_sessions.remove(session_id);
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Custom auth session has expired; restart the auth flow",
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
        return Err(AwsError::bad_request(
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
