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
                "event": super::cognito_trigger_event(event, trigger_source, &ctx.region),
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

/// Resolve a `Username` parameter to the canonical username key the
/// pool stores against. Real Cognito's `AdminGetUser` accepts either
/// the literal username (a UUID for native users, `<provider>_<id>`
/// for federated users) or the user's `sub`. We try the literal key
/// first, then scan for a matching sub. Returns `None` if neither
/// hits — callers raise `UserNotFoundException`.
pub(crate) fn resolve_username(pool: &UserPool, identifier: &str) -> Option<String> {
    if pool.users.contains_key(identifier) {
        return Some(identifier.to_string());
    }
    if let Some(name) = pool
        .users
        .iter()
        .find_map(|(name, user)| (user.sub == identifier).then(|| name.clone()))
    {
        return Some(name);
    }
    // Real AWS also accepts an alias (email / phone_number / preferred_username)
    // as the Username on the admin and code APIs.
    resolve_via_attributes(pool, identifier)
}

/// Match an identifier against a pool's alias and username attributes,
/// honouring Cognito's verified requirement: `email` / `phone_number` aliases
/// resolve only when the matching `<attr>_verified` flag is "true", while
/// `preferred_username` is always usable. Returns the stored user-pool key.
fn resolve_via_attributes(pool: &UserPool, input: &str) -> Option<String> {
    let mut aliases: Vec<&String> = Vec::new();
    for a in pool
        .alias_attributes
        .iter()
        .chain(pool.username_attributes.iter())
    {
        if !aliases.contains(&a) {
            aliases.push(a);
        }
    }
    for alias in aliases {
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
            if !matches {
                continue;
            }
            let verified_ok = match alias.as_str() {
                "email" => {
                    user.attributes.get("email_verified").map(String::as_str) == Some("true")
                }
                "phone_number" => {
                    user.attributes
                        .get("phone_number_verified")
                        .map(String::as_str)
                        == Some("true")
                }
                _ => true,
            };
            if verified_ok {
                return Some(key.clone());
            }
        }
    }
    None
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
                "LimitExceededException",
                "Attempt limit exceeded, please try after some time.",
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
            "LimitExceededException",
            "Attempt limit exceeded, please try after some time.",
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

/// Mask an email the way Cognito does in CodeDeliveryDetails, e.g.
/// `jane@example.com` -> `j***@e***.com`. Non-email or empty input is
/// returned masked as best-effort.
fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {
            let local_first = local.chars().next().unwrap();
            let dom_first = domain.chars().next().unwrap();
            match domain.rsplit_once('.') {
                Some((_, tld)) => format!("{local_first}***@{dom_first}***.{tld}"),
                None => format!("{local_first}***@{dom_first}***"),
            }
        }
        _ => "***".to_string(),
    }
}

pub fn user_to_value(user: &CognitoUser) -> Value {
    let attributes: Vec<Value> = user
        .attributes
        .iter()
        .map(|(k, v)| json!({"Name": k, "Value": v}))
        .collect();

    // Surface the user's enrolled MFA methods and preference, derived from the
    // per-user MFA state, the way AdminGetUser / GetUser report them.
    let mut mfa_settings: Vec<&str> = Vec::new();
    if user.software_token_mfa_enabled {
        mfa_settings.push("SOFTWARE_TOKEN_MFA");
    }
    if user.sms_mfa_enabled && user.attributes.contains_key("phone_number") {
        mfa_settings.push("SMS_MFA");
    }
    let preferred = user.mfa_preferred.clone().unwrap_or_default();

    // This is the UserType shape (ListUsers / AdminCreateUser / group lists),
    // which uses `Attributes`. AdminGetUser / GetUser report `UserAttributes`
    // and build that key themselves.
    json!({
        "Username": user.username,
        "UserStatus": user.status,
        "Enabled": user.enabled,
        "UserCreateDate": user.created_date,
        "UserLastModifiedDate": user.last_modified_date,
        "Attributes": &attributes,
        "UserMFASettingList": mfa_settings,
        "PreferredMfaSetting": preferred
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
        last_modified_date: now_epoch(),
        pending_verifications: HashMap::new(),
        pending_verifications_issued: HashMap::new(),
        code_failed_attempts: 0,
        code_locked_until_secs: None,
        revoked_refresh_tokens: Vec::new(),
        signed_out_at: None,
        sms_mfa_enabled: false,
        software_token_mfa_enabled: false,
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

/// Parse `UserAttributes` (or similar) in either of the two shapes
/// Cognito clients have produced over time:
///
/// 1. JSON array: `[{Name, Value}, ...]` — the standard json-1.1 shape.
/// 2. Indexed object: `{"1": {Name, Value}, "5": {...}}` or
///    `{"member.1": {...}, "member.5": {...}}` — what older AWS query
///    serializers emit when forced through a json bridge. Indices may
///    be sparse; entries are gathered in ascending numeric order so
///    "last write wins" behaves the same as for the array form.
///
/// Any entry missing `Name` or `Value` (string) is silently dropped,
/// matching the array-form behavior.
fn parse_user_attributes(input: &Value, key: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let entries: Vec<&Value> = match &input[key] {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(obj) => {
            let mut indexed: Vec<(u64, &Value)> = Vec::with_capacity(obj.len());
            for (k, v) in obj {
                let idx_str = k.strip_prefix("member.").unwrap_or(k.as_str());
                if let Ok(idx) = idx_str.parse::<u64>() {
                    indexed.push((idx, v));
                }
            }
            indexed.sort_by_key(|(i, _)| *i);
            indexed.into_iter().map(|(_, v)| v).collect()
        }
        _ => Vec::new(),
    };
    for attr in entries {
        if let (Some(k), Some(v)) = (attr["Name"].as_str(), attr["Value"].as_str()) {
            attrs.insert(k.to_string(), v.to_string());
        }
    }
    attrs
}

/// Whether the app client `client_id` restricts attribute reads.
/// Returns the configured `ReadAttributes` set only when it is
/// non-empty. `None` means the AWS default (every attribute readable):
/// either no custom set was configured, or the token carried no
/// resolvable client (older / hand-rolled tokens). Used by the
/// access-token `GetUser` path and ID-token minting; `AdminGetUser`
/// never calls it and so is unrestricted, matching real Cognito.
pub(crate) fn client_read_set(pool: &UserPool, client_id: &str) -> Option<Vec<String>> {
    pool.clients
        .get(client_id)
        .map(|c| c.read_attributes.clone())
        .filter(|s| !s.is_empty())
}

/// Enforce the app client's `WriteAttributes` on the access-token
/// write paths (`UpdateUserAttributes` / `DeleteUserAttributes`).
/// With a custom (non-empty) set, every attribute the caller writes or
/// deletes must be a member, else Cognito returns
/// `NotAuthorizedException`. An empty set is the AWS default (all
/// mutable attributes) and an unresolvable client is unrestricted -
/// neither adds a constraint. `Admin*` APIs don't call this, so they
/// bypass it as in real Cognito.
fn enforce_write_attributes<'a>(
    pool: &UserPool,
    client_id: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<(), AwsError> {
    let Some(client) = pool.clients.get(client_id) else {
        return Ok(());
    };
    if client.write_attributes.is_empty() {
        return Ok(());
    }
    for name in names {
        if !client.write_attributes.iter().any(|w| w == name) {
            return Err(AwsError::bad_request(
                "NotAuthorizedException",
                "A client attempted to write unauthorized attribute",
            ));
        }
    }
    Ok(())
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
/// the pool; a collision with another user's matching attribute raises
/// `AliasExistsException` there, versus `UsernameExistsException` on
/// the UsernameAttributes path.
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
        ensure_attribute_unique(pool, username, ua, username, "UsernameExistsException")?;
    }

    for alias in &pool.alias_attributes {
        if pool.username_attributes.contains(alias) {
            continue;
        }
        if let Some(value) = attrs.get(alias) {
            ensure_attribute_unique(pool, username, alias, value, "AliasExistsException")?;
        }
    }

    Ok(attrs)
}

fn ensure_attribute_unique(
    pool: &UserPool,
    new_username: &str,
    attr: &str,
    value: &str,
    code: &str,
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
        let display = if attr == "phone_number" {
            "phone number"
        } else {
            attr
        };
        return Err(AwsError::bad_request(
            code,
            format!("An account with the given {display} already exists."),
        ));
    }
    Ok(())
}

/// Resolve a sign-in identifier (Username from `InitiateAuth` /
/// `AdminInitiateAuth` / hosted UI) to the actual user-pool key.
///
/// Tries a literal match against `pool.users` first, then resolves via the
/// pool's alias / username attributes. `email` and `phone_number` aliases
/// only resolve when the matching attribute is verified, so an unverified
/// (potentially squatted) address can't be used to sign in.
pub fn resolve_username_for_signin(pool: &UserPool, input: &str) -> Option<String> {
    if pool.users.contains_key(input) {
        return Some(input.to_string());
    }
    resolve_via_attributes(pool, input)
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
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let password = input["Password"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Password is required")
    })?;
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
                AwsError::service_not_found("ResourceNotFoundException", "User pool not found")
            })?
        }
        None => {
            return Err(AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("No user pool found for client: {client_id}"),
            ));
        }
    };

    if pool.users.contains_key(username) {
        return Err(AwsError::bad_request(
            "UsernameExistsException",
            "User already exists",
        ));
    }

    super::auth_policy::validate_password(&pool.policies, password)?;

    let raw_attrs = parse_user_attributes(input, "UserAttributes");
    validate_attribute_values(&pool.schema, &raw_attrs)?;
    validate_required_present(&pool.schema, &raw_attrs)?;
    let attributes = prepare_user_attributes(&pool, username, raw_attrs)?;
    let user = make_user(&pool.id, username, password, attributes, "UNCONFIRMED")?;
    let sub = user.sub.clone();

    // Pre Sign-Up trigger (fire-and-forget). Carry the user's real attributes.
    if let Some(arn) = pool.lambda_config.get("PreSignUp") {
        let user_attrs: serde_json::Map<String, Value> = user
            .attributes
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        let trigger_event = json!({
            "userPoolId": pool.id,
            "userName": username,
            "callerContext": { "clientId": client_id },
            "request": { "userAttributes": user_attrs, "validationData": {} }
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

    let email_dest = user.attributes.get("email").map(|e| mask_email(e));

    info!(username = %username, pool_id = %pool.id, "Cognito: user signed up");
    pool.users.insert(username.to_string(), user);

    let mut resp = json!({
        "UserSub": sub,
        "UserConfirmed": false,
    });
    // Only report delivery when there is an email to verify, with a masked
    // destination rather than a bare "***".
    if let Some(dest) = email_dest {
        resp["CodeDeliveryDetails"] = json!({
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": dest,
        });
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// ConfirmSignUp
// ---------------------------------------------------------------------------

pub fn confirm_sign_up(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
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
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("No user pool found for client: {client_id}"),
        )
    })?;

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let code_key = format!("{pool_id}:{username}");
    let auto_verified = pool.auto_verified_attributes.clone();
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
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
                "Invalid code provided, please request a code again.",
            ));
        }
        if provided != expected {
            return Err(record_code_failure(
                user,
                AwsError::bad_request(
                    "CodeMismatchException",
                    "Invalid verification code provided, please try again.",
                ),
            ));
        }
    } else if !input["ConfirmationCode"].is_null() {
        let provided = input["ConfirmationCode"].as_str().unwrap_or("");
        if provided.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "ConfirmationCode is required",
            ));
        }
    }

    record_code_success(user);
    let _ = state.confirmation_codes.remove(&code_key);
    let _ = state.confirmation_codes_issued.remove(&code_key);

    user.status = "CONFIRMED".to_string();
    user.last_modified_date = now_epoch();
    // AutoVerifiedAttributes on the pool flips the matching
    // `<attr>_verified` flag the moment the user confirms sign-up.
    // Without this the user is CONFIRMED but their email/phone never
    // shows up as verified, and downstream services that depend on
    // those flags (token claims, ListUsers filters) reject the user.
    for attr in &auto_verified {
        let flag = format!("{attr}_verified");
        user.attributes.insert(flag, "true".to_string());
    }
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    user.status = "CONFIRMED".to_string();
    user.last_modified_date = now_epoch();
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

/// Generate a random temporary password that satisfies the default pool
/// policy (>= 8 chars with a lowercase, uppercase, digit, and symbol). Used
/// when AdminCreateUser is called without an explicit TemporaryPassword,
/// instead of a guessable constant.
fn generate_temp_password() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const DIGITS: &[u8] = b"23456789";
    const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+";
    let pick = |rng: &mut rand::rngs::ThreadRng, set: &[u8]| set[rng.gen_range(0..set.len())];
    // One from each class guarantees the policy is met, then fill to length.
    let mut chars: Vec<u8> = vec![
        pick(&mut rng, LOWER),
        pick(&mut rng, UPPER),
        pick(&mut rng, DIGITS),
        pick(&mut rng, SYMBOLS),
    ];
    let all = [LOWER, UPPER, DIGITS, SYMBOLS].concat();
    while chars.len() < 14 {
        chars.push(pick(&mut rng, &all));
    }
    // Shuffle so the class-ordered prefix isn't predictable.
    for i in (1..chars.len()).rev() {
        chars.swap(i, rng.gen_range(0..=i));
    }
    String::from_utf8(chars).expect("ascii bytes are valid utf8")
}

pub fn admin_create_user(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let message_action = input["MessageAction"].as_str();

    let generated_password;
    let password = match input["TemporaryPassword"].as_str() {
        Some(p) => p,
        None => {
            generated_password = generate_temp_password();
            &generated_password
        }
    };

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    // MessageAction=RESEND re-sends the invitation for an existing user
    // instead of creating one: a missing user is an error, an existing one is
    // returned unchanged.
    if message_action == Some("RESEND") {
        let resolved = resolve_username(&pool, username);
        return match resolved.and_then(|u| pool.users.get(&u).map(user_to_value)) {
            Some(user_value) => {
                info!(username = %username, pool_id = %pool_id, "Cognito: admin re-sent user invitation");
                Ok(json!({ "User": user_value }))
            }
            None => Err(AwsError::service_not_found(
                "UserNotFoundException",
                "User does not exist.",
            )),
        };
    }

    if pool.users.contains_key(username) {
        return Err(AwsError::bad_request(
            "UsernameExistsException",
            "User account already exists.",
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
    // Unless suppressed, send the invitation (username + temporary password)
    // to the user's email via SES.
    if message_action != Some("SUPPRESS")
        && let Some(email) = user.attributes.get("email").map(String::from)
    {
        let (subject, body) =
            super::email::invitation_message(&pool.extra_config, username, password);
        super::email::deliver(ctx, &email, &subject, &body, "AdminCreateUser");
    }
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    if pool.users.remove(&username).is_none() {
        return Err(AwsError::service_not_found(
            "UserNotFoundException",
            "User does not exist.",
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    // AdminGetUser's response shape uses `UserAttributes`, not the UserType
    // `Attributes` member.
    let mut out = user_to_value(user);
    if let Some(map) = out.as_object_mut()
        && let Some(attrs) = map.remove("Attributes")
    {
        map.insert("UserAttributes".to_string(), attrs);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// AdminSetUserPassword
// ---------------------------------------------------------------------------

pub fn admin_set_user_password(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let password = input["Password"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Password is required")
    })?;
    // AWS defaults Permanent to false: an omitted flag means a temporary
    // password that the user must change on next sign-in.
    let permanent = input["Permanent"].as_bool().unwrap_or(false);

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    super::auth_policy::validate_password(&pool.policies, password)?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    user.password_hash = crate::password::hash(password)?;
    let (s, v) = crate::password::srp_material(pool_id, &username, password);
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
    user.last_modified_date = now_epoch();
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
    use awsim_core::pagination::{cap_max_results, paginate};

    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    // Collect and sort users for deterministic, resumable pagination.
    let mut users: Vec<CognitoUser> = pool.users.values().cloned().collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));

    // Apply Filter if provided. Cognito only supports `=` and `^=`; anything
    // else (notably `!=`) is rejected rather than silently mis-parsed.
    if let Some(filter_str) = input["Filter"].as_str()
        && !filter_str.trim().is_empty()
    {
        validate_cognito_filter(filter_str)?;
        users.retain(|u| evaluate_cognito_filter(u, filter_str));
    }

    let limit = cap_max_results(input["Limit"].as_i64(), 60, 60);
    let token = input["PaginationToken"].as_str();
    let page = paginate(users, limit, token, |u| u.username.clone())?;

    let user_values: Vec<Value> = page.items.iter().map(user_to_value).collect();

    let mut resp = json!({ "Users": user_values });
    if let Some(next) = page.next_token {
        resp["PaginationToken"] = json!(next);
    }
    Ok(resp)
}

/// Evaluate a Cognito ListUsers filter expression against a user.
///
/// Cognito filter format: `attribute operator "value"`
/// Operators: `=` (exact match), `^=` (starts with)
/// Reject a ListUsers filter using an operator Cognito does not support.
/// Only `=` and `^=` are valid; `!=` and a bare attribute (no operator) are
/// InvalidParameterException, matching the service.
fn validate_cognito_filter(filter: &str) -> Result<(), AwsError> {
    if filter.contains("^=") {
        return Ok(());
    }
    // A `=` that is preceded by `!` is the unsupported `!=` operator.
    if let Some(idx) = filter.find('=') {
        if filter[..idx].trim_end().ends_with('!') {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                "Invalid filter: the != operator is not supported.",
            ));
        }
        return Ok(());
    }
    Err(AwsError::bad_request(
        "InvalidParameterException",
        "Invalid filter: expected attribute = \"value\".",
    ))
}

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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let claims = crate::jwt::verify_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;
    let username = claims.username;

    for pool_entry in state.user_pools.iter() {
        if let Some(user) = pool_entry.users.get(&username) {
            // Filter to the app client's ReadAttributes (empty/missing
            // = AWS default, every attribute readable).
            let read_set = client_read_set(&pool_entry, &claims.client_id);
            let attributes: Vec<Value> = user
                .attributes
                .iter()
                .filter(|(k, _)| {
                    read_set
                        .as_ref()
                        .is_none_or(|set| set.iter().any(|a| a == *k))
                })
                .map(|(k, v)| json!({"Name": k, "Value": v}))
                .collect();

            return Ok(json!({
                "Username": user.username,
                "UserAttributes": attributes
            }));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
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
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
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
            AwsError::service_not_found(
                "ResourceNotFoundException",
                format!("User pool {pool_id} does not exist."),
            )
        })?;
        let lambda_arn = pool.lambda_config.get("CustomMessage").cloned();
        let resolved = resolve_username(&pool, username).ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?;
        let user = pool.users.get_mut(&resolved).ok_or_else(|| {
            AwsError::service_not_found("UserNotFoundException", "User does not exist.")
        })?;
        user.pending_verifications
            .insert(FORGOT_PASSWORD_KEY.to_string(), code.clone());
        user.pending_verifications_issued
            .insert(FORGOT_PASSWORD_KEY.to_string(), now_epoch());
        let real_email = user.attributes.get("email").cloned();
        dest = real_email
            .clone()
            .unwrap_or_else(|| "***@example.com".to_string());
        info!(
            username = %username,
            pool_id = %pool_id,
            "Cognito: ForgotPassword code issued (dev: also visible at /cognito/<pool>/oauth2/forgot-password/confirm)"
        );
        // Route the reset code to SES so it shows up in the sent-email store.
        if let Some(email) = real_email.as_deref() {
            let (subject, body) = super::email::forgot_password_message(&pool.extra_config, &code);
            super::email::deliver(ctx, email, &subject, &body, "ForgotPassword");
        }
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
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let password = input["Password"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Password is required")
    })?;
    let confirmation_code = input["ConfirmationCode"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ConfirmationCode is required")
    })?;
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
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    let mut pool = state.user_pools.get_mut(&pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;
    super::auth_policy::validate_password(&pool.policies, password)?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    check_code_rate_limit(user)?;

    let expected = user
        .pending_verifications
        .get(FORGOT_PASSWORD_KEY)
        .cloned()
        .ok_or_else(|| {
            AwsError::bad_request(
                "ExpiredCodeException",
                "Invalid code provided, please request a code again.",
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
            "Invalid code provided, please request a code again.",
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
    let (s, v) = crate::password::srp_material(&pool_id, &username, password);
    user.srp_salt = Some(s);
    user.srp_verifier = Some(v);
    user.status = "CONFIRMED".to_string();
    user.last_modified_date = now_epoch();
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let previous = input["PreviousPassword"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "PreviousPassword is required")
    })?;
    let proposed = input["ProposedPassword"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ProposedPassword is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            super::auth_policy::validate_password(&pool_entry.policies, proposed)?;
            let pool_id = pool_entry.id.clone();
            let user = pool_entry.users.get_mut(&username).ok_or_else(|| {
                AwsError::service_not_found("UserNotFoundException", "User does not exist.")
            })?;
            if !crate::password::verify(previous, &user.password_hash) {
                return Err(AwsError::bad_request(
                    "NotAuthorizedException",
                    "Incorrect username or password.",
                ));
            }
            user.password_hash = crate::password::hash(proposed)?;
            let (s, v) = crate::password::srp_material(&pool_id, &username, proposed);
            user.srp_salt = Some(s);
            user.srp_verifier = Some(v);
            user.failed_login_attempts = 0;
            user.locked_until_secs = None;
            user.last_modified_date = now_epoch();
            return Ok(json!({}));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    state
        .revoked_tokens
        .revoked
        .insert(access_token.to_string(), ());

    // Invalidate the caller's outstanding refresh tokens by stamping the
    // sign-out time on their user record; tokens minted before now stop
    // refreshing while a fresh sign-in still works.
    if let Some(claims) = crate::jwt::verify_access_token(access_token) {
        let now = now_epoch();
        for mut pool in state.user_pools.iter_mut() {
            if pool.clients.contains_key(&claims.client_id) {
                if let Some(user) = pool.users.get_mut(&claims.username) {
                    user.signed_out_at = Some(now);
                }
                break;
            }
        }
    }

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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    user.enabled = true;
    user.last_modified_date = now_epoch();
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    user.enabled = false;
    user.last_modified_date = now_epoch();
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    user.status = "RESET_REQUIRED".to_string();
    // AWS delivers a reset code with this call so the user can complete
    // ConfirmForgotPassword immediately; stash one under the forgot-password
    // key (matching what ForgotPassword writes) rather than dead-ending.
    let code = generate_reset_code();
    user.pending_verifications
        .insert(FORGOT_PASSWORD_KEY.to_string(), code.clone());
    user.pending_verifications_issued
        .insert(FORGOT_PASSWORD_KEY.to_string(), now_epoch());
    user.last_modified_date = now_epoch();
    info!(username = %username, pool_id = %pool_id, code = %code, "Cognito: admin reset user password");
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username_attrs = pool.username_attributes.clone();
    let schema = pool.schema.clone();
    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
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
    // A caller may set an email/phone and its verified flag in the same
    // request. The explicit flag must win, so only auto-reset the flag when
    // the caller did not supply it. (new_attrs is unordered, so this can't
    // depend on iteration order.)
    let caller_set_email_verified = new_attrs.contains_key("email_verified");
    let caller_set_phone_verified = new_attrs.contains_key("phone_number_verified");
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
        if k == "email"
            && !caller_set_email_verified
            && user.attributes.get("email").map(String::as_str) != Some(v.as_str())
        {
            user.attributes
                .insert("email_verified".to_string(), "false".to_string());
        }
        if k == "phone_number"
            && !caller_set_phone_verified
            && user.attributes.get("phone_number").map(String::as_str) != Some(v.as_str())
        {
            user.attributes
                .insert("phone_number_verified".to_string(), "false".to_string());
        }
        user.attributes.insert(k, v);
    }
    user.last_modified_date = now_epoch();
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let schema = pool.schema.clone();
    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let claims = crate::jwt::verify_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;
    let username = claims.username;

    let new_attrs = parse_user_attributes(input, "UserAttributes");

    for mut pool_entry in state.user_pools.iter_mut() {
        if pool_entry.users.contains_key(&username) {
            // Enforce the app client's WriteAttributes before any
            // mutation (access-token path; Admin* bypasses).
            enforce_write_attributes(
                &pool_entry,
                &claims.client_id,
                new_attrs.keys().map(String::as_str),
            )?;
            let username_attrs = pool_entry.username_attributes.clone();
            let schema = pool_entry.schema.clone();
            let user = pool_entry.users.get_mut(&username).expect("just checked");
            validate_attribute_values(&schema, &new_attrs)?;
            validate_mutability(&schema, &user.attributes, &new_attrs)?;
            apply_attribute_updates(user, new_attrs, &username_attrs)?;
            return Ok(json!({ "CodeDeliveryDetailsList": [] }));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let claims = crate::jwt::verify_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;
    let username = claims.username;

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
            // Deleting an attribute is a write - same WriteAttributes
            // gate (access-token path; Admin* bypasses).
            enforce_write_attributes(
                &pool_entry,
                &claims.client_id,
                attr_names.iter().map(String::as_str),
            )?;
            let schema = pool_entry.schema.clone();
            validate_deletable_names(&schema, &attr_names)?;
            let user = pool_entry.users.get_mut(&username).expect("just checked");
            for name in &attr_names {
                user.attributes.remove(name);
            }
            return Ok(json!({}));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

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

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
    ))
}

// ---------------------------------------------------------------------------
// ResendConfirmationCode
// ---------------------------------------------------------------------------

pub fn resend_confirmation_code(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
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
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    let pool = state.user_pools.get(&pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;
    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    let real_email = pool
        .users
        .get(&username)
        .and_then(|u| u.attributes.get("email").cloned());
    let dest = real_email
        .as_deref()
        .map(mask_email)
        .unwrap_or_else(|| "***".to_string());

    let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
    let key = format!("{pool_id}:{username}");
    state.confirmation_codes.insert(key.clone(), code.clone());
    state.confirmation_codes_issued.insert(key, now_epoch());

    if let Some(email) = real_email.as_deref() {
        let (subject, body) = super::email::verification_message(&pool.extra_config, &code);
        super::email::deliver(ctx, email, &subject, &body, "ResendConfirmationCode");
    }

    info!(username = %username, "Cognito: resend confirmation code");
    Ok(json!({
        "CodeDeliveryDetails": {
            "AttributeName": "email",
            "DeliveryMedium": "EMAIL",
            "Destination": dest
        }
    }))
}

// ---------------------------------------------------------------------------
// GetUserAttributeVerificationCode
// ---------------------------------------------------------------------------

pub fn get_user_attribute_verification_code(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let attribute_name = input["AttributeName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AttributeName is required")
    })?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

    for mut pool_entry in state.user_pools.iter_mut() {
        if let Some(user) = pool_entry.users.get_mut(&username) {
            let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
            user.pending_verifications
                .insert(attribute_name.to_string(), code.clone());
            user.pending_verifications_issued
                .insert(attribute_name.to_string(), now_epoch());
            let real_value = user.attributes.get(attribute_name).cloned();
            let dest = real_value
                .as_deref()
                .map(mask_email)
                .unwrap_or_else(|| "***".to_string());
            // Email attributes are routed to SES; phone numbers would be SMS,
            // which awsim does not model.
            if attribute_name == "email"
                && let Some(email) = real_value.as_deref()
            {
                let (subject, body) =
                    super::email::verification_message(&pool_entry.extra_config, &code);
                super::email::deliver(
                    ctx,
                    email,
                    &subject,
                    &body,
                    "GetUserAttributeVerificationCode",
                );
            }
            info!(username = %username, attribute_name = %attribute_name, "Cognito: attribute verification code sent");
            return Ok(json!({
                "CodeDeliveryDetails": {
                    "AttributeName": attribute_name,
                    "DeliveryMedium": "EMAIL",
                    "Destination": dest
                }
            }));
        }
    }

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let access_token = input["AccessToken"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AccessToken is required")
    })?;
    let attribute_name = input["AttributeName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "AttributeName is required")
    })?;
    let _code = input["Code"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Code is required"))?;

    if state.revoked_tokens.revoked.contains_key(access_token) {
        return Err(AwsError::bad_request(
            "NotAuthorizedException",
            "Access Token has been revoked",
        ));
    }

    let username = crate::jwt::extract_username_from_access_token(access_token)
        .ok_or_else(|| AwsError::bad_request("NotAuthorizedException", "Invalid Access Token"))?;

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
                        "Invalid code provided, please request a code again.",
                    ));
                }
                if _code != expected {
                    return Err(record_code_failure(
                        user,
                        AwsError::bad_request(
                            "CodeMismatchException",
                            "Invalid verification code provided, please try again.",
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

    Err(AwsError::service_not_found(
        "UserNotFoundException",
        "User does not exist.",
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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get_mut(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;

    for token in &user.revoked_refresh_tokens {
        state.revoked_tokens.revoked.insert(token.clone(), ());
    }
    user.revoked_refresh_tokens.clear();
    user.signed_out_at = Some(now_epoch());

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
        .ok_or_else(|| AwsError::bad_request("InvalidParameterException", "Token is required"))?;
    let _client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

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
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let username = input["Username"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "Username is required")
    })?;
    let max_results = input["MaxResults"].as_u64().unwrap_or(60).clamp(1, 60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let username = resolve_username(&pool, username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
    })?;
    let user = pool.users.get(&username).ok_or_else(|| {
        AwsError::service_not_found("UserNotFoundException", "User does not exist.")
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

    #[test]
    fn user_attributes_parser_handles_array_form() {
        let input = serde_json::json!({
            "UserAttributes": [
                { "Name": "email", "Value": "a@b" },
                { "Name": "name", "Value": "Ada" },
            ]
        });
        let got = parse_user_attributes(&input, "UserAttributes");
        assert_eq!(got.get("email").map(String::as_str), Some("a@b"));
        assert_eq!(got.get("name").map(String::as_str), Some("Ada"));
    }

    #[test]
    fn user_attributes_parser_accepts_sparse_numeric_indices() {
        let input = serde_json::json!({
            "UserAttributes": {
                "1": { "Name": "email", "Value": "a@b" },
                "5": { "Name": "name", "Value": "Ada" },
            }
        });
        let got = parse_user_attributes(&input, "UserAttributes");
        assert_eq!(got.len(), 2);
        assert_eq!(got.get("email").map(String::as_str), Some("a@b"));
        assert_eq!(got.get("name").map(String::as_str), Some("Ada"));
    }

    #[test]
    fn user_attributes_parser_accepts_member_dot_n_keys() {
        let input = serde_json::json!({
            "UserAttributes": {
                "member.2": { "Name": "email", "Value": "later@x" },
                "member.1": { "Name": "email", "Value": "first@x" },
                "member.7": { "Name": "name", "Value": "Ada" },
            }
        });
        let got = parse_user_attributes(&input, "UserAttributes");
        // member.2 sorts after member.1 so its email wins.
        assert_eq!(got.get("email").map(String::as_str), Some("later@x"));
        assert_eq!(got.get("name").map(String::as_str), Some("Ada"));
    }

    #[test]
    fn user_attributes_parser_skips_non_numeric_object_keys() {
        let input = serde_json::json!({
            "UserAttributes": {
                "garbage": { "Name": "email", "Value": "ignored@x" },
                "3": { "Name": "name", "Value": "Ada" },
            }
        });
        let got = parse_user_attributes(&input, "UserAttributes");
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("name").map(String::as_str), Some("Ada"));
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
            last_modified_date: 0,
            pending_verifications: Default::default(),
            pending_verifications_issued: Default::default(),
            code_failed_attempts: 0,
            code_locked_until_secs: None,
            revoked_refresh_tokens: Vec::new(),
            signed_out_at: None,
            sms_mfa_enabled: false,
            software_token_mfa_enabled: false,
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
        assert_eq!(err.code, "LimitExceededException");
        assert!(user.code_locked_until_secs.is_some());

        // Subsequent attempts are rejected by the rate-limit gate even if
        // the caller would have provided the right code.
        let err = check_code_rate_limit(&mut user).unwrap_err();
        assert_eq!(err.code, "LimitExceededException");
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
                    read_attributes: Vec::new(),
                    write_attributes: Vec::new(),
                    prevent_user_existence_errors: "LEGACY".into(),
                    enable_token_revocation: true,
                    auth_session_validity: 3,
                    allowed_oauth_flows_user_pool_client: false,
                    default_redirect_uri: None,
                    token_validity_units: None,
                    last_modified_date: 0,
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
        // A standard attribute marked Required (custom attributes cannot be
        // required in Cognito). Creating a user without it is rejected.
        create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Schema": [
                    { "Name": "email", "Required": true, "Mutable": true }
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
        assert!(err.message.contains("email"));
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
        assert_eq!(err.code, "InvalidParameterException");
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

    #[test]
    fn admin_get_user_accepts_sub_as_username() {
        // Captify regression: clients pass the user's `sub` (UUID) as
        // the AdminGetUser `Username` parameter. AWS docs explicitly
        // allow this when `sub` isn't an alias attribute on the pool.
        let (state, pool_id) = schema_fixture();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "alice",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [{ "Name": "custom:plan", "Value": "x" }]
            }),
            &ctx(),
        )
        .unwrap();

        // Find Alice's auto-generated sub.
        let sub = state
            .user_pools
            .get(&pool_id)
            .unwrap()
            .users
            .get("alice")
            .unwrap()
            .sub
            .clone();

        // AdminGetUser by sub should succeed and return Alice.
        let by_sub = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": sub }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(by_sub["Username"], "alice");

        // Username that's neither a real username nor a known sub
        // still raises UserNotFoundException.
        let err = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "nope" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "UserNotFoundException");
    }

    // -----------------------------------------------------------------
    // App-client Read/WriteAttributes enforcement on the access-token
    // user APIs (config lives on the client; enforced here at runtime).
    // -----------------------------------------------------------------

    fn token_for(pool_id: &str) -> String {
        crate::jwt::access_token(
            "sub-u1",
            "us-east-1",
            pool_id,
            "c1",
            "u1",
            &[],
            &[],
            None,
            3600,
            None,
        )
    }

    fn set_client_attrs(state: &CognitoState, pool_id: &str, read: &[&str], write: &[&str]) {
        state.user_pools.alter(pool_id, |_, mut pool| {
            if let Some(c) = pool.clients.get_mut("c1") {
                c.read_attributes = read.iter().map(|s| s.to_string()).collect();
                c.write_attributes = write.iter().map(|s| s.to_string()).collect();
            }
            pool
        });
    }

    fn make_u1(state: &CognitoState, pool_id: &str) {
        admin_create_user(
            state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "TemporaryPassword": "Temp@1234",
                "UserAttributes": [
                    { "Name": "email", "Value": "u1@example.com" },
                    { "Name": "custom:plan", "Value": "enterprise" }
                ]
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn attr_names(v: &Value) -> std::collections::HashSet<String> {
        v["UserAttributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["Name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn get_user_filters_to_client_read_attributes() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let token = token_for(&pool_id);

        // Empty ReadAttributes = AWS default: every attribute returned.
        let all = get_user(&state, &json!({ "AccessToken": token }), &ctx()).unwrap();
        let names = attr_names(&all);
        assert!(names.contains("email"));
        assert!(names.contains("custom:plan"));

        // A custom set restricts the response to exactly that set.
        set_client_attrs(&state, &pool_id, &["email"], &[]);
        let filtered = get_user(&state, &json!({ "AccessToken": token }), &ctx()).unwrap();
        let names = attr_names(&filtered);
        assert!(names.contains("email"));
        assert!(
            !names.contains("custom:plan"),
            "custom:plan must be filtered out, got {names:?}"
        );
    }

    #[test]
    fn update_user_attributes_enforces_write_set() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let token = token_for(&pool_id);

        // Empty WriteAttributes = AWS default: any mutable attr writes.
        update_user_attributes(
            &state,
            &json!({ "AccessToken": token, "UserAttributes": [{ "Name": "email", "Value": "new@example.com" }] }),
            &ctx(),
        )
        .unwrap();

        // Restrict writes to custom:plan only.
        set_client_attrs(&state, &pool_id, &[], &["custom:plan"]);
        update_user_attributes(
            &state,
            &json!({ "AccessToken": token, "UserAttributes": [{ "Name": "custom:plan", "Value": "pro" }] }),
            &ctx(),
        )
        .unwrap();
        let err = update_user_attributes(
            &state,
            &json!({ "AccessToken": token, "UserAttributes": [{ "Name": "email", "Value": "x@example.com" }] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");
    }

    #[test]
    fn delete_user_attributes_enforces_write_set() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let token = token_for(&pool_id);

        set_client_attrs(&state, &pool_id, &[], &["email"]);
        // Deleting custom:plan is a write outside the set -> rejected.
        let err = delete_user_attributes(
            &state,
            &json!({ "AccessToken": token, "UserAttributeNames": ["custom:plan"] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "NotAuthorizedException");

        // Allowed once the set includes it.
        set_client_attrs(&state, &pool_id, &[], &["custom:plan"]);
        delete_user_attributes(
            &state,
            &json!({ "AccessToken": token, "UserAttributeNames": ["custom:plan"] }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn admin_update_bypasses_client_write_set() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        // Client forbids writing email...
        set_client_attrs(&state, &pool_id, &[], &["custom:plan"]);
        // ...but Admin* APIs use AWS creds, not the client, so this
        // still succeeds (matches real Cognito).
        admin_update_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributes": [{ "Name": "email", "Value": "admin-set@example.com" }]
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn admin_reset_user_password_stores_reset_code() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        admin_reset_user_password(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1" }),
            &ctx(),
        )
        .unwrap();
        let pool = state.user_pools.get(&pool_id).unwrap();
        let user = pool.users.get("u1").unwrap();
        assert_eq!(user.status, "RESET_REQUIRED");
        // A reset code is now available so ConfirmForgotPassword can proceed.
        assert!(user.pending_verifications.contains_key(FORGOT_PASSWORD_KEY));
    }

    #[test]
    fn list_users_rejects_not_equals_filter() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let err = list_users(
            &state,
            &json!({ "UserPoolId": pool_id, "Filter": "email != \"x@y.com\"" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn admin_get_user_resolves_verified_email_alias() {
        // AliasAttributes=email pool: a user keyed by username with a
        // verified email can be fetched by that email; an unverified one
        // cannot.
        let state = CognitoState::default();
        create_user_pool(
            &state,
            &json!({ "PoolName": "p", "AliasAttributes": ["email"] }),
            &ctx(),
        )
        .unwrap();
        let pool_id = state.user_pools.iter().next().unwrap().id.clone();
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "user-uuid",
                "MessageAction": "SUPPRESS",
                "UserAttributes": [
                    { "Name": "email", "Value": "dave@example.com" },
                    { "Name": "email_verified", "Value": "true" }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let by_email = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "dave@example.com" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(by_email["Username"], "user-uuid");

        // A second user with an unverified email is not resolvable by it.
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "user2-uuid",
                "MessageAction": "SUPPRESS",
                "UserAttributes": [{ "Name": "email", "Value": "unverified@example.com" }]
            }),
            &ctx(),
        )
        .unwrap();
        let err = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "unverified@example.com" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "UserNotFoundException");
    }

    #[test]
    fn mask_email_matches_cognito_shape() {
        assert_eq!(mask_email("jane@example.com"), "j***@e***.com");
        assert_eq!(mask_email("x@y.io"), "x***@y***.io");
        assert_eq!(mask_email("garbage"), "***");
    }

    #[test]
    fn admin_get_user_uses_user_attributes_key() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let got = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1" }),
            &ctx(),
        )
        .unwrap();
        // AdminGetUser uses UserAttributes, not the UserType Attributes member.
        assert!(got.get("UserAttributes").is_some());
        assert!(got.get("Attributes").is_none());
    }

    #[test]
    fn admin_get_user_reports_mfa_fields() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        // Before enrollment: empty list, empty preference.
        let before = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(before["UserMFASettingList"].as_array().unwrap().len(), 0);
        assert_eq!(before["PreferredMfaSetting"], "");
        // Enroll a verified software token + preference directly.
        {
            let mut pool = state.user_pools.get_mut(&pool_id).unwrap();
            let user = pool.users.get_mut("u1").unwrap();
            user.totp_verified = true;
            user.software_token_mfa_enabled = true;
            user.mfa_preferred = Some("SOFTWARE_TOKEN_MFA".to_string());
        }
        let after = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1" }),
            &ctx(),
        )
        .unwrap();
        let list: Vec<String> = after["UserMFASettingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(list.contains(&"SOFTWARE_TOKEN_MFA".to_string()));
        assert_eq!(after["PreferredMfaSetting"], "SOFTWARE_TOKEN_MFA");
    }

    #[test]
    fn admin_create_user_resend_missing_user_errors() {
        let (state, pool_id) = schema_fixture();
        let err = admin_create_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "ghost", "MessageAction": "RESEND" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "UserNotFoundException");
    }

    #[test]
    fn admin_create_user_resend_existing_returns_user() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        let res = admin_create_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1", "MessageAction": "RESEND" }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(res["User"]["Username"], "u1");
    }

    #[test]
    fn admin_create_user_generates_policy_compliant_temp_password() {
        let (state, pool_id) = schema_fixture();
        // No TemporaryPassword: a random one is generated and must pass the
        // pool policy (so the call succeeds rather than erroring).
        admin_create_user(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "auto",
                "UserAttributes": [{ "Name": "email", "Value": "auto@example.com" }]
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn admin_update_explicit_email_verified_is_honored() {
        let (state, pool_id) = schema_fixture();
        make_u1(&state, &pool_id);
        // Set a new email and email_verified=true in the same request: the
        // explicit flag must win over the auto-unverify, deterministically.
        admin_update_user_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "Username": "u1",
                "UserAttributes": [
                    { "Name": "email", "Value": "new@example.com" },
                    { "Name": "email_verified", "Value": "true" }
                ]
            }),
            &ctx(),
        )
        .unwrap();
        let got = admin_get_user(
            &state,
            &json!({ "UserPoolId": pool_id, "Username": "u1" }),
            &ctx(),
        )
        .unwrap();
        let verified = got["UserAttributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["Name"] == "email_verified")
            .map(|a| a["Value"].clone());
        assert_eq!(verified, Some(json!("true")));
    }
}
