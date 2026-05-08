use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::operations::schema_validation::{
    validate_attribute_values, validate_deletable_names, validate_mutability,
    validate_required_present,
};
use crate::state::{CognitoState, CognitoUser, UserPool};

/// Fire-and-forget Lambda trigger via the event bus.
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

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Default validity window for confirmation / reset codes, matching real
/// Cognito's 24-hour expiry.
const CODE_VALIDITY_SECS: u64 = 24 * 3600;

/// Returns true when `issued_at` is within `CODE_VALIDITY_SECS` of now.
/// Missing timestamps (legacy entries before the expiry was added) are
/// treated as expired so a stale code from a snapshot can't be replayed.
fn code_still_valid(issued_at: Option<u64>) -> bool {
    match issued_at {
        Some(ts) => now_epoch().saturating_sub(ts) < CODE_VALIDITY_SECS,
        None => false,
    }
}

/// How many consecutive wrong codes we tolerate before locking the user
/// out of code submission for [`CODE_LOCKOUT_SECS`].
const CODE_ATTEMPT_LIMIT: u32 = 5;
/// Length of the cool-off applied once a user crosses the attempt limit.
/// 15 minutes matches Cognito's documented behaviour of throttling brute
/// force attempts on its 6-digit codes.
const CODE_LOCKOUT_SECS: u64 = 15 * 60;

/// Reject the request if the user is currently locked out from submitting
/// codes; otherwise return Ok and let the caller verify the code itself.
fn check_code_rate_limit(user: &mut CognitoUser) -> Result<(), AwsError> {
    if let Some(until) = user.code_locked_until_secs {
        if now_epoch() < until {
            return Err(AwsError::bad_request(
                "TooManyRequestsException",
                "Too many attempts in a short period of time; try again later.",
            ));
        }
        // Cool-off elapsed; clear so the user can try again.
        user.code_locked_until_secs = None;
        user.code_failed_attempts = 0;
    }
    Ok(())
}

/// Record a failed code attempt and engage the cool-off if we cross the
/// attempt limit. Always returns the original `mismatch_err` (or the
/// rate-limit error once the lockout fires) so callers can `?` it.
fn record_code_failure(user: &mut CognitoUser, mismatch_err: AwsError) -> AwsError {
    user.code_failed_attempts = user.code_failed_attempts.saturating_add(1);
    if user.code_failed_attempts >= CODE_ATTEMPT_LIMIT {
        user.code_locked_until_secs = Some(now_epoch() + CODE_LOCKOUT_SECS);
        return AwsError::bad_request(
            "TooManyRequestsException",
            "Too many attempts in a short period of time; try again later.",
        );
    }
    mismatch_err
}

/// Reset the failure counter and any pending lockout after a successful
/// code consumption.
fn record_code_success(user: &mut CognitoUser) {
    user.code_failed_attempts = 0;
    user.code_locked_until_secs = None;
}

pub fn user_to_value(user: &CognitoUser) -> Value {
    let attributes: Vec<Value> = user
        .attributes
        .iter()
        .map(|(k, v)| json!({"Name": k, "Value": v}))
        .collect();

    json!({
        "Username": user.username,
        "UserStatus": user.status,
        "Enabled": user.enabled,
        "UserCreateDate": user.created_date,
        "UserLastModifiedDate": user.created_date,
        "Attributes": &attributes,
        "UserAttributes": &attributes
    })
}

fn make_user(
    pool_id: &str,
    username: &str,
    password: &str,
    attributes: HashMap<String, String>,
    status: &str,
) -> Result<CognitoUser, AwsError> {
    let sub = Uuid::new_v4().to_string();
    let mut attrs = attributes;
    attrs.insert("sub".to_string(), sub.clone());
    let (salt_hex, verifier_hex) = crate::password::srp_material(pool_id, username, password);
    Ok(CognitoUser {
        username: username.to_string(),
        sub,
        password_hash: crate::password::hash(password)?,
        srp_salt: Some(salt_hex),
        srp_verifier: Some(verifier_hex),
        attributes: attrs,
        status: status.to_string(),
        enabled: true,
        groups: Vec::new(),
        created_date: now_epoch(),
        pending_verifications: HashMap::new(),
        pending_verifications_issued: HashMap::new(),
        code_failed_attempts: 0,
        code_locked_until_secs: None,
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
    })
}

fn parse_user_attributes(input: &Value, key: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    if let Some(arr) = input[key].as_array() {
        for attr in arr {
            if let (Some(k), Some(v)) = (attr["Name"].as_str(), attr["Value"].as_str()) {
                attrs.insert(k.to_string(), v.to_string());
            }
        }
    }
    attrs
}

/// Validate a Username + caller-provided attributes against a pool's
/// `UsernameAttributes` / `AliasAttributes` config and return the
/// effective attribute map.
///
/// When `UsernameAttributes` includes `email`/`phone_number`:
///   * the Username must be a valid value of that attribute (basic
///     `@`-shape check for email),
///   * the corresponding attribute is force-set from Username,
///   * any caller-supplied conflicting value is overwritten,
///   * any other user already holding that attribute value triggers
///     `UsernameExistsException` so a copy-paste seed bug surfaces at
///     create time instead of at login time.
///
/// When `AliasAttributes` is set (and we're not already pinning via
/// UsernameAttributes), the alias value must be globally unique within
/// the pool — same `UsernameExistsException` if it collides with
/// another user's matching attribute.
fn prepare_user_attributes(
    pool: &UserPool,
    username: &str,
    mut attrs: HashMap<String, String>,
) -> Result<HashMap<String, String>, AwsError> {
    for ua in &pool.username_attributes {
        match ua.as_str() {
            "email" => {
                if !looks_like_email(username) {
                    return Err(AwsError::bad_request(
                        "InvalidParameterException",
                        "Username must be a valid email address",
                    ));
                }
                attrs.insert("email".to_string(), username.to_string());
            }
            "phone_number" => {
                if !looks_like_phone(username) {
                    return Err(AwsError::bad_request(
                        "InvalidParameterException",
                        "Username must be a valid E.164 phone number",
                    ));
                }
                attrs.insert("phone_number".to_string(), username.to_string());
            }
            _ => {}
        }
        ensure_attribute_unique(pool, username, ua, username)?;
    }

    for alias in &pool.alias_attributes {
        if pool.username_attributes.contains(alias) {
            continue;
        }
        if let Some(value) = attrs.get(alias) {
            ensure_attribute_unique(pool, username, alias, value)?;
        }
    }

    Ok(attrs)
}

fn ensure_attribute_unique(
    pool: &UserPool,
    new_username: &str,
    attr: &str,
    value: &str,
) -> Result<(), AwsError> {
    let case_insensitive = matches!(attr, "email" | "preferred_username");
    let needle = if case_insensitive {
        value.to_ascii_lowercase()
    } else {
        value.to_string()
    };
    let collision = pool.users.iter().any(|(u, user)| {
        if u == new_username {
            return false;
        }
        let Some(existing) = user.attributes.get(attr) else {
            return false;
        };
        if case_insensitive {
            existing.eq_ignore_ascii_case(&needle)
        } else {
            existing == &needle
        }
    });
    if collision {
        return Err(AwsError::conflict(
            "UsernameExistsException",
            format!("An account with the given {attr} already exists"),
        ));
    }
    Ok(())
}

/// Resolve a sign-in identifier (Username from `InitiateAuth` /
/// `AdminInitiateAuth` / hosted UI) to the actual user-pool key.
///
/// First tries a literal match against `pool.users`. If that misses and
/// the pool has `AliasAttributes` configured, scans users for one whose
/// matching attribute equals the input (case-insensitive for `email` and
/// `preferred_username`). Returns `None` if no user is found by either
/// route.
pub fn resolve_username_for_signin(pool: &UserPool, input: &str) -> Option<String> {
    if pool.users.contains_key(input) {
        return Some(input.to_string());
    }
    if pool.alias_attributes.is_empty() {
        return None;
    }
    for alias in &pool.alias_attributes {
        let case_insensitive = matches!(alias.as_str(), "email" | "preferred_username");
        for (key, user) in pool.users.iter() {
            let Some(stored) = user.attributes.get(alias) else {
                continue;
            };
            let matches = if case_insensitive {
                stored.eq_ignore_ascii_case(input)
            } else {
                stored == input
            };
            if matches {
                return Some(key.clone());
            }
        }
    }
    None
}

fn looks_like_email(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn looks_like_phone(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 8 && bytes[0] == b'+' && bytes[1..].iter().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// SignUp
// ---------------------------------------------------------------------------

pub fn sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        input["SecretHash"].as_str(),
        username,
    )?;

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let mut pool = match pool_entry {
        Some(e) => {
            let pool_id = e.id.clone();
            drop(e);
            state.user_pools.get_mut(&pool_id).ok_or_else(|| {
                AwsError::not_found("ResourceNotFoundException", "User pool not found")
            })?
        }
        None => {
            return Err(AwsError::not_found(
                "ResourceNotFoundException",
                format!("No user pool found for client: {client_id}"),
            ));
        }
    };

    if pool.users.contains_key(username) {
        return Err(AwsError::conflict(
            "UsernameExistsException",
            format!("Username already exists: {username}"),
        ));
    }

    super::auth_policy::validate_password(&pool.policies, password)?;

    let raw_attrs = parse_user_attributes(input, "UserAttributes");
    validate_attribute_values(&pool.schema, &raw_attrs)?;
    validate_required_present(&pool.schema, &raw_attrs)?;
    let attributes = prepare_user_attributes(&pool, username, raw_attrs)?;
    let user = make_user(&pool.id, username, password, attributes, "UNCONFIRMED")?;
    let sub = user.sub.clone();

    // Pre Sign-Up trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PreSignUp") {
        let trigger_event = json!({
            "userPoolId": pool.id,
            "userName": username,
            "callerContext": { "clientId": client_id },
            "request": { "userAttributes": {} }
        });
        invoke_trigger(ctx, "PreSignUp_SignUp", arn, &trigger_event);
    }

    // Custom Message trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("CustomMessage") {
        let trigger_event = json!({
            "userPoolId": pool.id,
            "userName": username,
            "triggerSource": "CustomMessage_SignUp"
        });
        invoke_trigger(ctx, "CustomMessage_SignUp", arn, &trigger_event);
    }

    info!(username = %username, pool_id = %pool.id, "Cognito: user signed up");
    pool.users.insert(username.to_string(), user);

    Ok(json!({
        "UserSub": sub,
        "UserConfirmed": false,
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": "***"
        }
    }))
}

// ---------------------------------------------------------------------------
// ConfirmSignUp
// ---------------------------------------------------------------------------

pub fn confirm_sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        input["SecretHash"].as_str(),
        username,
    )?;

    let pool_entry = state
        .user_pools
        .iter()
        .find(|e| e.clients.contains_key(client_id));

    let pool_id = pool_entry.map(|e| e.id.clone()).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("No user pool found for client: {client_id}"),
        )
    })?;

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let code_key = format!("{pool_id}:{username}");
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    check_code_rate_limit(user)?;

    let stored = state
        .confirmation_codes
        .get(&code_key)
        .map(|e| e.value().clone());
    if let Some(expected) = stored {
        let provided = input["ConfirmationCode"].as_str().unwrap_or("");
        let issued = state
            .confirmation_codes_issued
            .get(&code_key)
            .map(|e| *e.value());
        if !code_still_valid(issued) {
            // Drop the stale entry so a retry isn't tempted to keep
            // probing the same code.
            state.confirmation_codes.remove(&code_key);
            state.confirmation_codes_issued.remove(&code_key);
            return Err(AwsError::bad_request(
                "ExpiredCodeException",
                "Confirmation code has expired",
            ));
        }
        if provided != expected {
            return Err(record_code_failure(
                user,
                AwsError::bad_request(
                    "CodeMismatchException",
                    "Invalid verification code provided",
                ),
            ));
        }
    } else if !input["ConfirmationCode"].is_null() {
        let provided = input["ConfirmationCode"].as_str().unwrap_or("");
        if provided.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "ConfirmationCode is required",
            ));
        }
    }

    record_code_success(user);
    let _ = state.confirmation_codes.remove(&code_key);
    let _ = state.confirmation_codes_issued.remove(&code_key);

    user.status = "CONFIRMED".to_string();
    info!(username = %username, "Cognito: user confirmed sign-up");

    // Post-Confirmation trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PostConfirmation") {
        let trigger_event = json!({
            "userPoolId": pool_id,
            "userName": username,
            "callerContext": { "clientId": client_id }
        });
        invoke_trigger(ctx, "PostConfirmation_ConfirmSignUp", arn, &trigger_event);
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminConfirmSignUp
// ---------------------------------------------------------------------------

pub fn admin_confirm_sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.status = "CONFIRMED".to_string();
    info!(username = %username, pool_id = %pool_id, "Cognito: admin confirmed sign-up");

    // Post-Confirmation trigger (fire-and-forget)
    if let Some(arn) = pool.lambda_config.get("PostConfirmation") {
        let trigger_event = json!({
            "userPoolId": pool_id,
            "userName": username,
        });
        invoke_trigger(ctx, "PostConfirmation_ConfirmSignUp", arn, &trigger_event);
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminCreateUser
// ---------------------------------------------------------------------------

pub fn admin_create_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let password = input["TemporaryPassword"].as_str().unwrap_or("Temp@1234");

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.users.contains_key(username) {
        return Err(AwsError::conflict(
            "UsernameExistsException",
            format!("Username already exists: {username}"),
        ));
    }

    super::auth_policy::validate_password(&pool.policies, password)?;

    let raw_attrs = parse_user_attributes(input, "UserAttributes");
    validate_attribute_values(&pool.schema, &raw_attrs)?;
    validate_required_present(&pool.schema, &raw_attrs)?;
    let attributes = prepare_user_attributes(&pool, username, raw_attrs)?;
    let user = make_user(
        &pool.id,
        username,
        password,
        attributes,
        "FORCE_CHANGE_PASSWORD",
    )?;
    let user_value = user_to_value(&user);
    info!(username = %username, pool_id = %pool_id, "Cognito: admin created user");
    pool.users.insert(username.to_string(), user);

    Ok(json!({ "User": user_value }))
}

// ---------------------------------------------------------------------------
// AdminDeleteUser
// ---------------------------------------------------------------------------

pub fn admin_delete_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.users.remove(username).is_none() {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    info!(username = %username, pool_id = %pool_id, "Cognito: admin deleted user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminGetUser
// ---------------------------------------------------------------------------

pub fn admin_get_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    Ok(user_to_value(user))
}

// ---------------------------------------------------------------------------
// AdminSetUserPassword
// ---------------------------------------------------------------------------

pub fn admin_set_user_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;
    let permanent = input["Permanent"].as_bool().unwrap_or(true);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    super::auth_policy::validate_password(&pool.policies, password)?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.password_hash = crate::password::hash(password)?;
    let (s, v) = crate::password::srp_material(pool_id, username, password);
    user.srp_salt = Some(s);
    user.srp_verifier = Some(v);
    // AWS semantics: Permanent=true => CONFIRMED, Permanent=false => the
    // password is treated as temporary and the user must change it on
    // next sign-in. We were previously only flipping to CONFIRMED on
    // Permanent=true and leaving the status alone otherwise, which let
    // a CONFIRMED user keep CONFIRMED status when given a temp password
    // — opposite of what AWS does.
    user.status = if permanent {
        "CONFIRMED".to_string()
    } else {
        "FORCE_CHANGE_PASSWORD".to_string()
    };
    // Setting a fresh password administratively unlocks the account.
    user.failed_login_attempts = 0;
    user.locked_until_secs = None;

    info!(username = %username, pool_id = %pool_id, permanent, "Cognito: admin set user password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListUsers
// ---------------------------------------------------------------------------

pub fn list_users(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    // Collect and sort users for deterministic pagination
    let mut users: Vec<&CognitoUser> = pool.users.values().collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));

    // Apply Filter if provided
    if let Some(filter_str) = input["Filter"].as_str() {
        users.retain(|u| evaluate_cognito_filter(u, filter_str));
    }

    // Apply PaginationToken — skip users up to and including the token username
    if let Some(token) = input["PaginationToken"].as_str()
        && let Some(pos) = users.iter().position(|u| u.username == token)
    {
        users = users.into_iter().skip(pos + 1).collect();
    }

    // Apply Limit
    let limit = input["Limit"].as_u64().unwrap_or(60) as usize;
    let has_more = users.len() > limit;
    let next_token = if has_more {
        users.get(limit - 1).map(|u| u.username.clone())
    } else {
        None
    };
    users.truncate(limit);

    let user_values: Vec<Value> = users.iter().map(|u| user_to_value(u)).collect();

    let mut resp = json!({ "Users": user_values });
    if let Some(token) = next_token {
        resp["PaginationToken"] = json!(token);
    }
    Ok(resp)
}

/// Evaluate a Cognito ListUsers filter expression against a user.
///
/// Cognito filter format: `attribute operator "value"`
/// Operators: `=` (exact match), `^=` (starts with)
fn evaluate_cognito_filter(user: &CognitoUser, filter: &str) -> bool {
    // Determine operator and split
    let (attr_name, operator, value) = if let Some(idx) = filter.find("^=") {
        (filter[..idx].trim(), "^=", filter[idx + 2..].trim())
    } else if let Some(idx) = filter.find('=') {
        (filter[..idx].trim(), "=", filter[idx + 1..].trim())
    } else {
        return true; // Unrecognised filter — pass all
    };

    // Strip surrounding quotes from value
    let value = value.trim_matches('"');

    let user_value: Option<&str> = match attr_name {
        "cognito:user_status" | "status" => Some(user.status.as_str()),
        "username" => Some(user.username.as_str()),
        "sub" => Some(user.sub.as_str()),
        "enabled" => Some(if user.enabled { "true" } else { "false" }),
        attr => user.attributes.get(attr).map(|s| s.as_str()),
    };

    match (user_value, operator) {
        (Some(v), "=") => v == value,
        (Some(v), "^=") => v.starts_with(value),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// GetUser (uses AccessToken)
// ---------------------------------------------------------------------------

pub fn get_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for pool_entry in state.user_pools.iter() {
        if let Some(user) = pool_entry.users.get(&username) {
            let attributes: Vec<Value> = user
                .attributes
                .iter()
                .map(|(k, v)| json!({"Name": k, "Value": v}))
                .collect();

            return Ok(json!({
                "Username": user.username,
                "UserAttributes": attributes
            }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// ForgotPassword
// ---------------------------------------------------------------------------

pub fn forgot_password(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        input["SecretHash"].as_str(),
        username,
    )?;

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

    // Generate + persist a 6-digit code so ConfirmForgotPassword has
    // something to validate against. We log it at info level so devs
    // can grab it from the awsim console — a real Cognito would email
    // it. Stashed under the existing `pending_verifications` map with
    // the conventional key `forgot_password`.
    let code = generate_reset_code();
    let dest;
    {
        let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
            AwsError::not_found(
                "ResourceNotFoundException",
                format!("User pool not found: {pool_id}"),
            )
        })?;
        let lambda_arn = pool.lambda_config.get("CustomMessage").cloned();
        let user = pool.users.get_mut(username).ok_or_else(|| {
            AwsError::not_found(
                "UserNotFoundException",
                format!("User not found: {username}"),
            )
        })?;
        user.pending_verifications
            .insert(FORGOT_PASSWORD_KEY.to_string(), code.clone());
        user.pending_verifications_issued
            .insert(FORGOT_PASSWORD_KEY.to_string(), now_epoch());
        dest = user
            .attributes
            .get("email")
            .cloned()
            .unwrap_or_else(|| "***@example.com".to_string());
        info!(
            username = %username,
            pool_id = %pool_id,
            code = %code,
            "Cognito: ForgotPassword code issued (dev: also visible at /cognito/<pool>/oauth2/forgot-password/confirm)"
        );
        // Custom Message trigger (fire-and-forget) — kept here so the
        // immutable Lambda ARN we cloned out is still in scope.
        if let Some(arn) = lambda_arn {
            let trigger_event = json!({
                "userPoolId": pool_id,
                "userName": username,
                "triggerSource": "CustomMessage_ForgotPassword",
                "codeParameter": code,
            });
            invoke_trigger(ctx, "CustomMessage_ForgotPassword", &arn, &trigger_event);
        }
    }

    Ok(json!({
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": dest
        }
    }))
}

/// Convention key for the stored ForgotPassword code on each user's
/// `pending_verifications` map.
pub(crate) const FORGOT_PASSWORD_KEY: &str = "forgot_password";

fn generate_reset_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000u32))
}

// ---------------------------------------------------------------------------
// ConfirmForgotPassword
// ---------------------------------------------------------------------------

pub fn confirm_forgot_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let password = input["Password"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Password is required"))?;
    let confirmation_code = input["ConfirmationCode"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ConfirmationCode is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        input["SecretHash"].as_str(),
        username,
    )?;

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

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    super::auth_policy::validate_password(&pool.policies, password)?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;
    check_code_rate_limit(user)?;

    let expected = user
        .pending_verifications
        .get(FORGOT_PASSWORD_KEY)
        .cloned()
        .ok_or_else(|| {
            AwsError::bad_request(
                "ExpiredCodeException",
                "No active forgot-password code for this user",
            )
        })?;
    let issued = user
        .pending_verifications_issued
        .get(FORGOT_PASSWORD_KEY)
        .copied();
    if !code_still_valid(issued) {
        user.pending_verifications.remove(FORGOT_PASSWORD_KEY);
        user.pending_verifications_issued
            .remove(FORGOT_PASSWORD_KEY);
        return Err(AwsError::bad_request(
            "ExpiredCodeException",
            "Forgot-password code has expired",
        ));
    }
    if expected != confirmation_code {
        return Err(record_code_failure(
            user,
            AwsError::bad_request(
                "CodeMismatchException",
                "Invalid verification code provided, please try again.",
            ),
        ));
    }

    record_code_success(user);
    user.pending_verifications.remove(FORGOT_PASSWORD_KEY);
    user.pending_verifications_issued
        .remove(FORGOT_PASSWORD_KEY);
    user.password_hash = crate::password::hash(password)?;
    let (s, v) = crate::password::srp_material(&pool_id, username, password);
    user.srp_salt = Some(s);
    user.srp_verifier = Some(v);
    user.status = "CONFIRMED".to_string();
    user.failed_login_attempts = 0;
    user.locked_until_secs = None;

    info!(username = %username, "Cognito: confirm forgot password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ChangePassword
// ---------------------------------------------------------------------------

pub fn change_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let previous = input["PreviousPassword"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PreviousPassword is required"))?;
    let proposed = input["ProposedPassword"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ProposedPassword is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            super::auth_policy::validate_password(&pool_entry.policies, proposed)?;
            let pool_id = pool_entry.id.clone();
            let user = pool_entry.users.get_mut(&username).ok_or_else(|| {
                AwsError::not_found(
                    "UserNotFoundException",
                    format!("User not found: {username}"),
                )
            })?;
            if !crate::password::verify(previous, &user.password_hash) {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect previous password",
                ));
            }
            user.password_hash = crate::password::hash(proposed)?;
            let (s, v) = crate::password::srp_material(&pool_id, &username, proposed);
            user.srp_salt = Some(s);
            user.srp_verifier = Some(v);
            user.failed_login_attempts = 0;
            user.locked_until_secs = None;
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// GlobalSignOut
// ---------------------------------------------------------------------------

pub fn global_sign_out(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    state
        .revoked_tokens
        .revoked
        .insert(access_token.to_string(), ());

    info!("Cognito: global sign out");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminEnableUser
// ---------------------------------------------------------------------------

pub fn admin_enable_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.enabled = true;
    info!(username = %username, pool_id = %pool_id, "Cognito: admin enabled user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminDisableUser
// ---------------------------------------------------------------------------

pub fn admin_disable_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.enabled = false;
    info!(username = %username, pool_id = %pool_id, "Cognito: admin disabled user");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminResetUserPassword
// ---------------------------------------------------------------------------

pub fn admin_reset_user_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    user.status = "RESET_REQUIRED".to_string();
    info!(username = %username, pool_id = %pool_id, "Cognito: admin reset user password");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminUpdateUserAttributes
// ---------------------------------------------------------------------------

pub fn admin_update_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let username_attrs = pool.username_attributes.clone();
    let schema = pool.schema.clone();
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let new_attrs = parse_user_attributes(input, "UserAttributes");
    validate_attribute_values(&schema, &new_attrs)?;
    validate_mutability(&schema, &user.attributes, &new_attrs)?;

    apply_attribute_updates(user, new_attrs, &username_attrs)?;

    info!(username = %username, pool_id = %pool_id, "Cognito: admin updated user attributes");
    Ok(json!({}))
}

/// Merge a set of attribute updates into a user, mirroring real Cognito.
///
/// Enforces:
/// - `sub` is read-only (`InvalidParameterException` on change).
/// - When the pool's `UsernameAttributes` includes the attribute being
///   changed, the attribute is the canonical Username and is therefore
///   read-only (matches AWS's "you can't change your email if email is
///   the username" rule).
/// - Mutating `email` / `phone_number` flips `_verified` back to
///   `false`.
fn apply_attribute_updates(
    user: &mut CognitoUser,
    new_attrs: HashMap<String, String>,
    pool_username_attributes: &[String],
) -> Result<(), AwsError> {
    for (k, v) in new_attrs {
        if k == "sub" {
            if user.attributes.get("sub").map(String::as_str) == Some(v.as_str()) {
                continue;
            }
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "user.sub is read-only",
            ));
        }
        if pool_username_attributes.iter().any(|a| a == &k)
            && user.attributes.get(&k).map(String::as_str) != Some(v.as_str())
        {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("{k} is the pool's username and cannot be changed"),
            ));
        }
        if k == "email" && user.attributes.get("email").map(String::as_str) != Some(v.as_str()) {
            user.attributes
                .insert("email_verified".to_string(), "false".to_string());
        }
        if k == "phone_number"
            && user.attributes.get("phone_number").map(String::as_str) != Some(v.as_str())
        {
            user.attributes
                .insert("phone_number_verified".to_string(), "false".to_string());
        }
        user.attributes.insert(k, v);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AdminDeleteUserAttributes
// ---------------------------------------------------------------------------

pub fn admin_delete_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let schema = pool.schema.clone();
    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    let names: Vec<String> = input["UserAttributeNames"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    validate_deletable_names(&schema, &names)?;
    for name in &names {
        user.attributes.remove(name);
    }

    info!(username = %username, pool_id = %pool_id, "Cognito: admin deleted user attributes");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// UpdateUserAttributes (authenticated user updates own attributes)
// ---------------------------------------------------------------------------

pub fn update_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    let new_attrs = parse_user_attributes(input, "UserAttributes");

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            let username_attrs = pool_entry.username_attributes.clone();
            let schema = pool_entry.schema.clone();
            let user = pool_entry.users.get_mut(&username).expect("just checked");
            validate_attribute_values(&schema, &new_attrs)?;
            validate_mutability(&schema, &user.attributes, &new_attrs)?;
            apply_attribute_updates(user, new_attrs, &username_attrs)?;
            return Ok(json!({ "CodeDeliveryDetailsList": [] }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// DeleteUserAttributes (authenticated user deletes own attributes)
// ---------------------------------------------------------------------------

pub fn delete_user_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    let attr_names: Vec<String> = input["UserAttributeNames"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            let schema = pool_entry.schema.clone();
            validate_deletable_names(&schema, &attr_names)?;
            let user = pool_entry.users.get_mut(&username).expect("just checked");
            for name in &attr_names {
                user.attributes.remove(name);
            }
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// DeleteUser (authenticated user deletes own account)
// ---------------------------------------------------------------------------

pub fn delete_user(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.remove(&username).is_some() {
            state
                .revoked_tokens
                .revoked
                .insert(access_token.to_string(), ());
            info!(username = %username, "Cognito: user deleted own account");
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// ResendConfirmationCode
// ---------------------------------------------------------------------------

pub fn resend_confirmation_code(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    crate::secret_hash::validate_for_client(
        state,
        client_id,
        input["SecretHash"].as_str(),
        username,
    )?;

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

    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;
    if !pool.users.contains_key(username) {
        return Err(AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        ));
    }

    let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
    let key = format!("{pool_id}:{username}");
    state.confirmation_codes.insert(key.clone(), code.clone());
    state.confirmation_codes_issued.insert(key, now_epoch());

    info!(username = %username, code = %code, "Cognito: resend confirmation code");
    Ok(json!({
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": "***"
        }
    }))
}

// ---------------------------------------------------------------------------
// GetUserAttributeVerificationCode
// ---------------------------------------------------------------------------

pub fn get_user_attribute_verification_code(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let attribute_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
            user.pending_verifications
                .insert(attribute_name.to_string(), code.clone());
            user.pending_verifications_issued
                .insert(attribute_name.to_string(), now_epoch());
            info!(username = %username, attribute_name = %attribute_name, code = %code, "Cognito: attribute verification code sent");
            return Ok(json!({
                "CodeDeliveryDetails": {
                    "AttributeName": attribute_name,
                    "DeliveryMedium": "EMAIL",
                    "Destination": "***"
                }
            }));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// VerifyUserAttribute
// ---------------------------------------------------------------------------

pub fn verify_user_attribute(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AccessToken is required"))?;
    let attribute_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;
    let _code = input["Code"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Code is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid access token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            check_code_rate_limit(user)?;
            if let Some(expected) = user.pending_verifications.get(attribute_name).cloned() {
                let issued = user
                    .pending_verifications_issued
                    .get(attribute_name)
                    .copied();
                if !code_still_valid(issued) {
                    user.pending_verifications.remove(attribute_name);
                    user.pending_verifications_issued.remove(attribute_name);
                    return Err(AwsError::bad_request(
                        "ExpiredCodeException",
                        "Attribute verification code has expired",
                    ));
                }
                if _code != expected {
                    return Err(record_code_failure(
                        user,
                        AwsError::bad_request(
                            "CodeMismatchException",
                            "Invalid verification code provided",
                        ),
                    ));
                }
            }
            record_code_success(user);
            let verified_key = format!("{attribute_name}_verified");
            user.attributes.insert(verified_key, "true".to_string());
            user.pending_verifications.remove(attribute_name);
            user.pending_verifications_issued.remove(attribute_name);
            info!(username = %username, attribute_name = %attribute_name, "Cognito: verified user attribute");
            return Ok(json!({}));
        }
    }

    Err(AwsError::not_found(
        "UserNotFoundException",
        format!("User not found: {username}"),
    ))
}

// ---------------------------------------------------------------------------
// AdminUserGlobalSignOut
// ---------------------------------------------------------------------------

pub fn admin_user_global_sign_out(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get_mut(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    for token in &user.revoked_refresh_tokens {
        state.revoked_tokens.revoked.insert(token.clone(), ());
    }
    user.revoked_refresh_tokens.clear();

    info!(username = %username, pool_id = %pool_id, "Cognito: admin global sign out");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// RevokeToken
// ---------------------------------------------------------------------------

pub fn revoke_token(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let token = input["Token"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Token is required"))?;
    let _client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    state.revoked_tokens.revoked.insert(token.to_string(), ());
    info!("Cognito: revoke token");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// AdminListUserAuthEvents
// ---------------------------------------------------------------------------

pub fn admin_list_user_auth_events(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let username = input["Username"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Username is required"))?;
    let max_results = input["MaxResults"].as_u64().unwrap_or(60).clamp(1, 60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let user = pool.users.get(username).ok_or_else(|| {
        AwsError::not_found(
            "UserNotFoundException",
            format!("User not found: {username}"),
        )
    })?;

    // Newest first per AWS semantics, capped at MaxResults.
    let events: Vec<Value> = user
        .auth_events
        .iter()
        .rev()
        .take(max_results)
        .map(|e| {
            json!({
                "EventId": e.event_id,
                "EventType": e.event_type,
                "CreationDate": e.creation_date,
                "EventResponse": e.event_response,
                "EventRisk": {
                    "RiskDecision": e.risk_decision,
                    "RiskLevel": e.risk_level,
                    "CompromisedCredentialsDetected": e.compromised_credentials_detected,
                },
                "EventFeedback": e.feedback_value.as_ref().map(|v| json!({
                    "FeedbackValue": v,
                    "Provider": "Cognito"
                })),
            })
        })
        .collect();

    Ok(json!({ "AuthEvents": events }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_just_issued_is_valid() {
        assert!(code_still_valid(Some(now_epoch())));
    }

    #[test]
    fn code_within_window_is_valid() {
        // 23 hours old: still inside the 24-hour window.
        assert!(code_still_valid(Some(now_epoch() - 23 * 3600)));
    }

    #[test]
    fn code_past_window_is_expired() {
        // 25 hours old: just past the cap.
        assert!(!code_still_valid(Some(now_epoch() - 25 * 3600)));
    }

    #[test]
    fn missing_timestamp_treated_as_expired() {
        // Legacy snapshot codes have no issued time; fail closed.
        assert!(!code_still_valid(None));
    }

    fn fixture_user() -> CognitoUser {
        CognitoUser {
            username: "u".into(),
            sub: "s".into(),
            password_hash: "x".into(),
            srp_salt: None,
            srp_verifier: None,
            attributes: Default::default(),
            status: "CONFIRMED".into(),
            enabled: true,
            groups: Vec::new(),
            created_date: 0,
            pending_verifications: Default::default(),
            pending_verifications_issued: Default::default(),
            code_failed_attempts: 0,
            code_locked_until_secs: None,
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
    fn rate_limit_engages_after_threshold_failures() {
        let mut user = fixture_user();
        let dummy = || AwsError::bad_request("CodeMismatchException", "Invalid verification code");

        // First (LIMIT - 1) failures bubble up the original mismatch error.
        for _ in 0..(CODE_ATTEMPT_LIMIT - 1) {
            let err = record_code_failure(&mut user, dummy());
            assert_eq!(err.code, "CodeMismatchException");
        }
        assert!(user.code_locked_until_secs.is_none());

        // The threshold-th failure flips the lockout.
        let err = record_code_failure(&mut user, dummy());
        assert_eq!(err.code, "TooManyRequestsException");
        assert!(user.code_locked_until_secs.is_some());

        // Subsequent attempts are rejected by the rate-limit gate even if
        // the caller would have provided the right code.
        let err = check_code_rate_limit(&mut user).unwrap_err();
        assert_eq!(err.code, "TooManyRequestsException");
    }

    #[test]
    fn rate_limit_clears_on_success() {
        let mut user = fixture_user();
        user.code_failed_attempts = 3;
        record_code_success(&mut user);
        assert_eq!(user.code_failed_attempts, 0);
        assert!(user.code_locked_until_secs.is_none());
    }

    #[test]
    fn rate_limit_releases_after_lockout_window() {
        let mut user = fixture_user();
        user.code_failed_attempts = CODE_ATTEMPT_LIMIT;
        // Lockout that already expired in the past.
        user.code_locked_until_secs = Some(now_epoch().saturating_sub(1));
        check_code_rate_limit(&mut user).expect("expired lockout should clear");
        assert!(user.code_locked_until_secs.is_none());
        assert_eq!(user.code_failed_attempts, 0);
    }

    // -----------------------------------------------------------------
    // Schema enforcement on user write paths.
    // -----------------------------------------------------------------

    use crate::operations::pools::{add_custom_attributes, create_user_pool};
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    /// Build a state + pool + admin-created confirmed user with a
    /// `custom:plan` String attr declared and one client. Returns
    /// (state, pool_id, client_id).
    fn schema_fixture() -> (CognitoState, String) {
        let state = CognitoState::default();
        let input = json!({
            "PoolName": "p",
            "Schema": [
                { "Name": "plan", "AttributeDataType": "String",
                  "StringAttributeConstraints": { "MinLength": "1", "MaxLength": "32" } },
                { "Name": "rank", "AttributeDataType": "Number",
                  "NumberAttributeConstraints": { "MinValue": "0", "MaxValue": "10" } },
                { "Name": "frozen", "AttributeDataType": "String", "Mutable": false }
            ]
        });
        create_user_pool(&state, &input, &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();
        // Add a client so SignUp has somewhere to land.
        state.user_pools.alter(&pool_id, |_, mut pool| {
            pool.clients.insert(
                "c1".to_string(),
                crate::state::UserPoolClient {
                    client_id: "c1".to_string(),
                    client_name: "test".to_string(),
                    user_pool_id: pool.id.clone(),
                    explicit_auth_flows: Vec::new(),
                    created_date: 0,
                    client_secret: None,
                    additional_client_secrets: Vec::new(),
                    callback_urls: Vec::new(),
                    logout_urls: Vec::new(),
                    allowed_oauth_flows: Vec::new(),
                    allowed_oauth_scopes: Vec::new(),
                    supported_identity_providers: Vec::new(),
                    access_token_validity: 3600,
                    id_token_validity: 3600,
                    refresh_token_validity: 30,
                },
            );
            pool
        });
        (state, pool_id)
    }

    #[test]
    fn admin_create_user_rejects_undeclared_custom_attr() {
        let (state, pool_id) = schema_fixture();
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:not_in_schema", "Value": "x" }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("does not exist in the schema"));
    }

    #[test]
    fn admin_create_user_with_declared_custom_attr_succeeds() {
        let (state, pool_id) = schema_fixture();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:plan", "Value": "enterprise" }
                ]
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn admin_create_user_rejects_bad_number_value() {
        let (state, pool_id) = schema_fixture();
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:rank", "Value": "high" }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("non-numeric"));
    }

    #[test]
    fn admin_create_user_rejects_out_of_range_number() {
        let (state, pool_id) = schema_fixture();
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:rank", "Value": "99" }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("greater than max"));
    }

    #[test]
    fn admin_create_user_rejects_overlong_string() {
        let (state, pool_id) = schema_fixture();
        let too_long = "x".repeat(33);
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:plan", "Value": too_long }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("longer than max"));
    }

    #[test]
    fn admin_create_user_required_attr_missing_rejected() {
        let state = CognitoState::default();
        // Required attr without default value.
        create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Schema": [
                    { "Name": "org", "AttributeDataType": "String", "Required": true }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234"
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("required attribute missing"));
        assert!(err.message.contains("custom:org"));
    }

    #[test]
    fn admin_update_user_attributes_rejects_undeclared() {
        let (state, pool_id) = schema_fixture();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234"
            }),
            &ctx(),
        )
        .unwrap();
        let err = admin_update_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributes": [
                    { "Name": "custom:not_in_schema", "Value": "x" }
                ]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn admin_update_user_attributes_rejects_change_of_immutable() {
        let (state, pool_id) = schema_fixture();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "custom:frozen", "Value": "v1" }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        // Same value should pass.
        admin_update_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributes": [{ "Name": "custom:frozen", "Value": "v1" }]
            }),
            &ctx(),
        )
        .unwrap();
        // Different value rejected.
        let err = admin_update_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributes": [{ "Name": "custom:frozen", "Value": "v2" }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn admin_delete_user_attributes_rejects_unknown_name() {
        let (state, pool_id) = schema_fixture();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234"
            }),
            &ctx(),
        )
        .unwrap();
        let err = admin_delete_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributeNames": ["custom:not_in_schema"]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("does not exist"));
    }

    #[test]
    fn add_custom_attributes_then_admin_create_user_succeeds() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();

        // Before AddCustomAttributes: rejected.
        let err = admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [{ "Name": "custom:plan", "Value": "x" }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");

        // Declare the attribute, then retry: ok.
        add_custom_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "CustomAttributes": [{ "Name": "plan", "AttributeDataType": "String" }]
            }),
            &ctx(),
        )
        .unwrap();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [{ "Name": "custom:plan", "Value": "x" }]
            }),
            &ctx(),
        )
        .unwrap();
    }
}
