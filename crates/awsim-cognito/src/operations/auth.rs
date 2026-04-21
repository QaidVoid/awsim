use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::jwt;
use crate::state::CognitoState;

fn build_auth_result(
    user_sub: &str,
    username: &str,
    region: &str,
    pool_id: &str,
    client_id: &str,
    attributes: &std::collections::HashMap<String, String>,
) -> Value {
    let id_tok = jwt::id_token(user_sub, region, pool_id, client_id, username, attributes);
    let access_tok = jwt::access_token(user_sub, region, pool_id, client_id, username);
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

            info!(username = %username, "Cognito: InitiateAuth success");
            Ok(build_auth_result(
                &user.sub,
                username,
                &ctx.region,
                &pool_id,
                client_id,
                &user.attributes,
            ))
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

            info!(username = %username, pool_id = %pool_id, "Cognito: AdminInitiateAuth success");
            Ok(build_auth_result(
                &user.sub,
                username,
                &ctx.region,
                pool_id,
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
