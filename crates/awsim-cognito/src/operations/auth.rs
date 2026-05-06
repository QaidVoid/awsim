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

pub fn build_auth_result_pub(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
    groups: &[GroupRolePair],
) -> Value {
    let validity = TokenValidity::defaults();
    build_auth_result_with_validity(
        user_sub, username, region, pool_id, client_id, attributes, groups, &validity,
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
    groups: &[GroupRolePair],
    validity: &TokenValidity,
) -> Value {
    build_auth_result_inner(
        user_sub, username, region, pool_id, client_id, attributes, groups, validity, true,
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
    groups: &[GroupRolePair],
    validity: &TokenValidity,
) -> Value {
    build_auth_result_with_validity(
        user_sub, username, region, pool_id, client_id, attributes, groups, validity,
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
        "USER_PASSWORD_AUTH" | "USER_SRP_AUTH" => {
            // For USER_SRP_AUTH we skip the SRP challenge and just do password auth
            let raw_username = params["USERNAME"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
            let password = params["PASSWORD"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PASSWORD is required"))?;
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
                super::auth_policy::check_not_locked(user)?;

                if user.password != password {
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
                super::auth_policy::check_not_locked(user)?;

                if user.password != password {
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
            user.password = new_password.to_string();
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
                &pairs,
                &validity,
            );

            info!(username = %username, "Cognito: RespondToAuthChallenge NEW_PASSWORD_REQUIRED success");
            Ok(result)
        }
        "SOFTWARE_TOKEN_MFA" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            if let Some(session) = state.mfa_sessions.remove(session_id) {
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
                    &pairs,
                    &validity,
                );
                info!(username = %session.1.username, "Cognito: RespondToAuthChallenge SOFTWARE_TOKEN_MFA success");
                Ok(result)
            } else {
                Ok(json!({ "AuthenticationResult": {} }))
            }
        }
        "MFA_SETUP" => Ok(json!({
            "ChallengeName": "MFA_SETUP",
            "ChallengeParameters": {},
            "Session": input["Session"]
        })),
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
            user.password = new_password.to_string();
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
                &pairs,
                &validity,
            );

            info!(username = %username, pool_id = %pool_id, "Cognito: AdminRespondToAuthChallenge NEW_PASSWORD_REQUIRED success");
            Ok(result)
        }
        "SOFTWARE_TOKEN_MFA" => {
            let session_id = input["Session"].as_str().unwrap_or("");
            if let Some(session) = state.mfa_sessions.remove(session_id) {
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
