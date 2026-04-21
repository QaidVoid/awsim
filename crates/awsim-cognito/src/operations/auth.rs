use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, MfaSession};
use crate::jwt;

pub fn build_auth_result_pub(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Value {
    // Use default openid scope for direct auth flows (InitiateAuth, etc.)
    let default_scopes: Vec<String> = vec!["openid".to_string(), "email".to_string(), "profile".to_string()];
    let id_tok = jwt::id_token(user_sub, region, pool_id, client_id, username, attributes, &default_scopes, None);
    let access_tok = jwt::access_token(user_sub, region, pool_id, client_id, username, &default_scopes);
    let refresh_tok = jwt::refresh_token(user_sub);

    json!({
        "AuthenticationResult": {
            "AccessToken": access_tok,
            "IdToken": id_tok,
            "RefreshToken": refresh_tok,
            "ExpiresIn": 3600,
            "TokenType": "Bearer"
        }
    })
}

fn build_auth_result(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Value {
    build_auth_result_pub(user_sub, username, region, pool_id, client_id, attributes)
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

    let pool_id = pool_entry
        .map(|e| e.id.clone())
        .ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("No pool found for client: {client_id}"),
            )
        })?;

    match auth_flow {
        "USER_PASSWORD_AUTH" | "USER_SRP_AUTH" => {
            // For USER_SRP_AUTH we skip the SRP challenge and just do password auth
            let username = params["USERNAME"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
            let password = params["PASSWORD"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PASSWORD is required"))?;

            let pool = state.user_pools.get(&pool_id).unwrap();

            // Pre-Authentication trigger (fire-and-forget)
            if let Some(arn) = pool.lambda_config.get("PreAuthentication") {
                let trigger_event = json!({
                    "userPoolId": pool_id,
                    "userName": username,
                    "callerContext": { "clientId": client_id }
                });
                invoke_trigger(ctx, "PreAuthentication_Authentication", arn, &trigger_event);
            }

            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found("UserNotFoundException", format!("User not found: {username}"))
            })?;

            if user.password != password {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect username or password",
                ));
            }

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed",
                ));
            }

            // Check whether MFA is required
            let mfa_required = pool.mfa_configuration == "ON"
                || (pool.mfa_configuration == "OPTIONAL" && user.mfa_enabled);

            if mfa_required && user.totp_verified {
                // Return SOFTWARE_TOKEN_MFA challenge
                let session_id = Uuid::new_v4().to_string();
                drop(user);
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
                invoke_trigger(ctx, "PostAuthentication_Authentication", arn, &trigger_event);
            }

            let result = build_auth_result(
                &user.sub,
                username,
                &ctx.region,
                &pool_id,
                client_id,
                &user.attributes,
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
                .and_then(|s| s.split('-').next())
                .unwrap_or("unknown");

            // Find user by sub
            let pool = state.user_pools.get(&pool_id).unwrap();
            let user = pool
                .users
                .values()
                .find(|u| u.sub == sub || refresh_tok.contains(&u.sub))
                .ok_or_else(|| {
                    AwsError::not_found("UserNotFoundException", "User not found for refresh token")
                })?;

            Ok(build_auth_result(
                &user.sub,
                &user.username,
                &ctx.region,
                &pool_id,
                client_id,
                &user.attributes,
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

    match auth_flow {
        "USER_PASSWORD_AUTH" | "ADMIN_USER_PASSWORD_AUTH" | "USER_SRP_AUTH" => {
            let username = params["USERNAME"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "USERNAME is required"))?;
            let password = params["PASSWORD"]
                .as_str()
                .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PASSWORD is required"))?;

            // Pre-Authentication trigger (fire-and-forget)
            if let Some(arn) = pool.lambda_config.get("PreAuthentication") {
                let trigger_event = json!({
                    "userPoolId": pool_id,
                    "userName": username,
                    "callerContext": { "clientId": client_id }
                });
                invoke_trigger(ctx, "PreAuthentication_Authentication", arn, &trigger_event);
            }

            let user = pool.users.get(username).ok_or_else(|| {
                AwsError::not_found("UserNotFoundException", format!("User not found: {username}"))
            })?;

            if user.password != password {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect username or password",
                ));
            }

            if user.status == "UNCONFIRMED" {
                return Err(AwsError::bad_request(
                    "UserNotConfirmedException",
                    "User is not confirmed",
                ));
            }

            // Check whether MFA is required
            let mfa_required = pool.mfa_configuration == "ON"
                || (pool.mfa_configuration == "OPTIONAL" && user.mfa_enabled);

            if mfa_required && user.totp_verified {
                let session_id = Uuid::new_v4().to_string();
                drop(user);
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
                invoke_trigger(ctx, "PostAuthentication_Authentication", arn, &trigger_event);
            }

            let result = build_auth_result(
                &user.sub,
                username,
                &ctx.region,
                pool_id,
                client_id,
                &user.attributes,
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
