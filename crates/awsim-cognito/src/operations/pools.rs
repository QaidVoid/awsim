use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, arn};
use rand::Rng;
use serde_json::{Value, json};
use tracing::info;

use crate::state::{
    CognitoState, MAX_CUSTOM_ATTRIBUTES, NumberAttributeConstraints, PasswordPolicy,
    SchemaAttribute, StringAttributeConstraints, UserPool, UserPoolClient,
    default_user_pool_schema,
};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a random alphanumeric string of `len` characters.
fn random_alnum(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            match idx {
                0..=25 => (b'a' + idx) as char,
                26..=51 => (b'A' + idx - 26) as char,
                _ => (b'0' + idx - 52) as char,
            }
        })
        .collect()
}

/// Generate a random 52-char alphanumeric client secret.
fn generate_client_secret() -> String {
    random_alnum(52)
}

fn password_policy_to_value(p: &PasswordPolicy) -> Value {
    json!({
        "MinimumLength": p.minimum_length,
        "RequireLowercase": p.require_lowercase,
        "RequireUppercase": p.require_uppercase,
        "RequireNumbers": p.require_numbers,
        "RequireSymbols": p.require_symbols,
        "TemporaryPasswordValidityDays": p.temporary_password_validity_days
    })
}

fn client_to_value(client: &UserPoolClient) -> Value {
    let mut obj = json!({
        "UserPoolId": client.user_pool_id,
        "ClientName": client.client_name,
        "ClientId": client.client_id,
        "ExplicitAuthFlows": client.explicit_auth_flows,
        "CallbackURLs": client.callback_urls,
        "LogoutURLs": client.logout_urls,
        "AllowedOAuthFlows": client.allowed_oauth_flows,
        "AllowedOAuthScopes": client.allowed_oauth_scopes,
        "AllowedOAuthFlowsUserPoolClient": client.allowed_oauth_flows_user_pool_client,
        "SupportedIdentityProviders": client.supported_identity_providers,
        "AccessTokenValidity": client.access_token_validity,
        "IdTokenValidity": client.id_token_validity,
        "RefreshTokenValidity": client.refresh_token_validity,
        "PreventUserExistenceErrors": client.prevent_user_existence_errors,
        "EnableTokenRevocation": client.enable_token_revocation,
        "AuthSessionValidity": client.auth_session_validity,
        "CreationDate": client.created_date,
        "LastModifiedDate": client.last_modified_date
    });
    // SAFETY: obj was created by json!() macro above, which always produces an object.
    let map = obj
        .as_object_mut()
        .expect("json!() macro always produces an object");
    // AWS returns ClientSecret from Create, Describe, and Update for a
    // confidential client; public clients simply have none.
    if let Some(secret) = &client.client_secret {
        map.insert("ClientSecret".into(), json!(secret));
    }
    if let Some(uri) = &client.default_redirect_uri {
        map.insert("DefaultRedirectURI".into(), json!(uri));
    }
    if let Some(units) = &client.token_validity_units {
        map.insert("TokenValidityUnits".into(), units.clone());
    }
    // Cognito only echoes Read/WriteAttributes when a custom set was
    // configured; an empty list means "the default set" and is omitted.
    if !client.read_attributes.is_empty() {
        map.insert("ReadAttributes".into(), json!(client.read_attributes));
    }
    if !client.write_attributes.is_empty() {
        map.insert("WriteAttributes".into(), json!(client.write_attributes));
    }
    obj
}

/// Validate a client's `ReadAttributes` / `WriteAttributes` against the
/// pool schema. Every referenced name must be a declared attribute, and
/// write targets must be mutable (Cognito rejects immutable attributes
/// such as `sub` for write). Empty lists are valid - they select the
/// AWS default set.
fn validate_client_attributes(
    pool: &UserPool,
    read: &[String],
    write: &[String],
) -> Result<(), AwsError> {
    let lookup = |name: &String| pool.schema.iter().find(|a| &a.name == name);
    for name in read.iter().chain(write.iter()) {
        if lookup(name).is_none() {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Attribute {name} does not exist in the user pool schema."),
            ));
        }
    }
    for name in write {
        if lookup(name).is_some_and(|a| !a.mutable) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Write attributes cannot include non-mutable attribute {name}."),
            ));
        }
    }
    Ok(())
}

/// Built-in identity providers always available to a user pool client.
const BUILTIN_IDPS: &[&str] = &[
    "COGNITO",
    "Google",
    "Facebook",
    "LoginWithAmazon",
    "SignInWithApple",
];

/// Standard OAuth scopes Cognito accepts without a resource server.
const STANDARD_OAUTH_SCOPES: &[&str] = &[
    "openid",
    "email",
    "phone",
    "profile",
    "aws.cognito.signin.user.admin",
];

/// Validate a client's OAuth-related fields against the pool. Callback /
/// logout URLs must be https (or http://localhost for development),
/// SupportedIdentityProviders must resolve to a built-in or a configured
/// provider, and AllowedOAuthScopes must be a standard scope or a declared
/// resource-server scope.
fn validate_client_oauth(
    pool: &UserPool,
    callback_urls: &[String],
    logout_urls: &[String],
    allowed_oauth_scopes: &[String],
    supported_identity_providers: &[String],
) -> Result<(), AwsError> {
    let url_ok = |u: &str| {
        u.starts_with("https://")
            || u.starts_with("http://localhost")
            || u.starts_with("http://127.0.0.1")
    };
    for u in callback_urls.iter().chain(logout_urls.iter()) {
        if !url_ok(u) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Redirect URI {u} must use https (or localhost for development)."),
            ));
        }
    }

    for idp in supported_identity_providers {
        let known = BUILTIN_IDPS.contains(&idp.as_str())
            || pool
                .identity_providers
                .iter()
                .any(|p| &p.provider_name == idp);
        if !known {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("The provider {idp} does not exist for the user pool."),
            ));
        }
    }

    for scope in allowed_oauth_scopes {
        let standard = STANDARD_OAUTH_SCOPES.contains(&scope.as_str());
        let resource = scope.split_once('/').is_some_and(|(rs_id, name)| {
            pool.resource_servers
                .iter()
                .any(|rs| rs.identifier == rs_id && rs.scopes.iter().any(|s| s.scope_name == name))
        });
        if !standard && !resource {
            return Err(AwsError::bad_request(
                "ScopeDoesNotExistException",
                format!("Scope {scope} does not exist."),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CreateUserPool
// ---------------------------------------------------------------------------

pub fn create_user_pool(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_name = input["PoolName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "PoolName is required")
    })?;

    let random = random_alnum(9);
    let pool_id = format!("{0}_{1}", ctx.region, random);
    let arn = arn::build(ctx, "cognito-idp", format!("userpool/{pool_id}"));
    let now = now_epoch();

    // Parse policies from input
    let policies = parse_password_policy(&input["Policies"]["PasswordPolicy"])?;
    let sign_in_policy_first_auth_factors =
        parse_sign_in_policy(&input["Policies"]["SignInPolicy"])?;

    let mfa_configuration = input["MfaConfiguration"]
        .as_str()
        .unwrap_or("OFF")
        .to_string();

    let auto_verified_attributes: Vec<String> = input["AutoVerifiedAttributes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let username_attributes = parse_string_list(&input["UsernameAttributes"]);
    let alias_attributes = parse_string_list(&input["AliasAttributes"]);

    // UsernameAttributes and AliasAttributes are mutually exclusive in AWS.
    if !username_attributes.is_empty() && !alias_attributes.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Only one of aliasAttributes or usernameAttributes can be set in a user pool.",
        ));
    }

    let lambda_config = parse_lambda_config(&input["LambdaConfig"]);
    let tags = parse_tag_map(&input["UserPoolTags"]);
    let extra_config = parse_extra_pool_config(input);

    let schema = build_initial_schema(&input["Schema"])?;

    let pool = UserPool {
        id: pool_id.clone(),
        name: pool_name.to_string(),
        arn: arn.clone(),
        clients: HashMap::new(),
        users: HashMap::new(),
        groups: HashMap::new(),
        created_date: now,
        policies,
        mfa_configuration,
        software_token_mfa_enabled: false,
        auto_verified_attributes,
        username_attributes,
        alias_attributes,
        lambda_config,
        schema,
        email_configuration: None,
        domain: None,
        resource_servers: Vec::new(),
        identity_providers: Vec::new(),
        tags,
        ui_customizations: HashMap::new(),
        managed_login_brandings: Vec::new(),
        risk_configurations: Vec::new(),
        import_jobs: Vec::new(),
        log_delivery_configuration: None,
        terms: Vec::new(),
        custom_auth_expected_answer: None,
        custom_auth_challenge_parameters: HashMap::new(),
        sign_in_policy_first_auth_factors,
        last_modified_date: now,
        extra_config,
    };

    info!(pool_id = %pool_id, "Cognito: created user pool");
    let response = json!({ "UserPool": pool_to_value(&pool) });
    state.user_pools.insert(pool_id.clone(), pool);

    Ok(response)
}

// ---------------------------------------------------------------------------
// DeleteUserPool
// ---------------------------------------------------------------------------

pub fn delete_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let removed = state.user_pools.remove(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    // Release any domain this pool claimed so the name can be reused and no
    // longer resolves to a dead pool.
    if let Some(domain) = &removed.1.domain {
        state.domain_pool_map.remove(domain);
    }

    info!(pool_id = %pool_id, "Cognito: deleted user pool");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeUserPool
// ---------------------------------------------------------------------------

pub fn describe_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    Ok(json!({ "UserPool": pool_to_value(&pool) }))
}

/// AWS LambdaConfig field names that carry a nested `{LambdaArn, LambdaVersion}`
/// object on the wire (we store them flattened as `<base>` + `<base>Version`).
const NESTED_LAMBDA_TRIGGERS: &[&str] =
    &["PreTokenGeneration", "CustomEmailSender", "CustomSMSSender"];

/// Reconstruct the wire-shape LambdaConfig from the flattened storage map.
fn lambda_config_to_value(cfg: &HashMap<String, String>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in cfg {
        if k.ends_with("Version") {
            continue; // folded into its base trigger's nested object below
        }
        if NESTED_LAMBDA_TRIGGERS.contains(&k.as_str()) {
            let mut nested = serde_json::Map::new();
            nested.insert("LambdaArn".to_string(), Value::String(v.clone()));
            if let Some(ver) = cfg.get(&format!("{k}Version")) {
                nested.insert("LambdaVersion".to_string(), Value::String(ver.clone()));
            }
            out.insert(format!("{k}Config"), Value::Object(nested));
        } else {
            out.insert(k.clone(), Value::String(v.clone()));
        }
    }
    Value::Object(out)
}

/// Build the full UserPoolType echoed by CreateUserPool / DescribeUserPool.
fn pool_to_value(pool: &UserPool) -> Value {
    let schema_attributes: Vec<Value> = pool.schema.iter().map(schema_attr_to_value).collect();
    let mut obj = json!({
        "Id": pool.id,
        "Name": pool.name,
        "Arn": pool.arn,
        "Status": "Active",
        "CreationDate": pool.created_date,
        "LastModifiedDate": pool.last_modified_date,
        "MfaConfiguration": pool.mfa_configuration,
        "AutoVerifiedAttributes": pool.auto_verified_attributes,
        "UsernameAttributes": pool.username_attributes,
        "AliasAttributes": pool.alias_attributes,
        "SchemaAttributes": schema_attributes,
        "LambdaConfig": lambda_config_to_value(&pool.lambda_config),
        "Policies": {
            "PasswordPolicy": password_policy_to_value(&pool.policies),
            "SignInPolicy": if pool.sign_in_policy_first_auth_factors.is_empty() {
                Value::Null
            } else {
                json!({ "AllowedFirstAuthFactors": pool.sign_in_policy_first_auth_factors })
            },
        },
        "UserPoolTags": pool.tags,
        "Domain": pool.domain,
        "EstimatedNumberOfUsers": pool.users.len()
    });
    // Echo the verbatim config blocks captured at create / update.
    if let Some(map) = obj.as_object_mut() {
        for (k, v) in &pool.extra_config {
            map.insert(k.clone(), v.clone());
        }
    }
    obj
}

// ---------------------------------------------------------------------------
// ListUserPools
// ---------------------------------------------------------------------------

pub fn list_user_pools(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use awsim_core::pagination::{cap_max_results, paginate};

    let mut pools: Vec<(String, String, u64, u64)> = state
        .user_pools
        .iter()
        .map(|e| {
            (
                e.id.clone(),
                e.name.clone(),
                e.created_date,
                e.last_modified_date,
            )
        })
        .collect();
    pools.sort_by(|a, b| a.0.cmp(&b.0));

    let limit = cap_max_results(input["MaxResults"].as_i64(), 60, 60);
    let token = input["NextToken"].as_str();
    let page = paginate(pools, limit, token, |p| p.0.clone())?;

    let user_pools: Vec<Value> = page
        .items
        .iter()
        .map(|(id, name, created, modified)| {
            json!({
                "Id": id,
                "Name": name,
                "Status": "Active",
                "CreationDate": created,
                "LastModifiedDate": modified
            })
        })
        .collect();

    let mut resp = json!({ "UserPools": user_pools });
    if let Some(next) = page.next_token {
        resp["NextToken"] = json!(next);
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// UpdateUserPool
// ---------------------------------------------------------------------------

pub fn update_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    if !input["Policies"]["PasswordPolicy"].is_null() {
        pool.policies = parse_password_policy(&input["Policies"]["PasswordPolicy"])?;
    }

    if let Some(mfa) = input["MfaConfiguration"].as_str() {
        pool.mfa_configuration = mfa.to_string();
    }

    if let Some(attrs) = input["AutoVerifiedAttributes"].as_array() {
        pool.auto_verified_attributes = attrs
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if !input["LambdaConfig"].is_null() {
        pool.lambda_config = parse_lambda_config(&input["LambdaConfig"]);
    }

    if !input["UserPoolTags"].is_null() {
        pool.tags = parse_tag_map(&input["UserPoolTags"]);
    }

    // Merge any supplied verbatim config blocks, leaving unspecified ones
    // untouched.
    for key in EXTRA_POOL_CONFIG_KEYS {
        if !input[*key].is_null() {
            pool.extra_config
                .insert((*key).to_string(), input[*key].clone());
        }
    }

    pool.last_modified_date = now_epoch();
    info!(pool_id = %pool_id, "Cognito: updated user pool");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// CreateUserPoolClient
// ---------------------------------------------------------------------------

pub fn create_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_name = input["ClientName"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientName is required")
    })?;

    let explicit_auth_flows: Vec<String> = match input["ExplicitAuthFlows"].as_array() {
        Some(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        // An unset ExplicitAuthFlows defaults to the SRP + custom + refresh
        // set, matching AWS (the password flows must be opted into).
        None => vec![
            "ALLOW_USER_SRP_AUTH".to_string(),
            "ALLOW_CUSTOM_AUTH".to_string(),
            "ALLOW_REFRESH_TOKEN_AUTH".to_string(),
        ],
    };

    let generate_secret = input["GenerateSecret"].as_bool().unwrap_or(false);
    let client_secret = if generate_secret {
        Some(generate_client_secret())
    } else {
        None
    };

    let callback_urls: Vec<String> = input["CallbackURLs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let logout_urls: Vec<String> = input["LogoutURLs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let allowed_oauth_flows: Vec<String> = input["AllowedOAuthFlows"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let allowed_oauth_scopes: Vec<String> = input["AllowedOAuthScopes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let supported_identity_providers: Vec<String> = input["SupportedIdentityProviders"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let read_attributes: Vec<String> = input["ReadAttributes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let write_attributes: Vec<String> = input["WriteAttributes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let access_token_validity = input["AccessTokenValidity"].as_u64().unwrap_or(3600);
    let id_token_validity = input["IdTokenValidity"].as_u64().unwrap_or(3600);
    let refresh_token_validity = input["RefreshTokenValidity"].as_u64().unwrap_or(2_592_000);

    let prevent_user_existence_errors = input["PreventUserExistenceErrors"]
        .as_str()
        .unwrap_or("LEGACY")
        .to_string();
    let enable_token_revocation = input["EnableTokenRevocation"].as_bool().unwrap_or(true);
    let auth_session_validity = input["AuthSessionValidity"].as_u64().unwrap_or(3) as u32;
    let allowed_oauth_flows_user_pool_client = input["AllowedOAuthFlowsUserPoolClient"]
        .as_bool()
        .unwrap_or(false);
    let default_redirect_uri = input["DefaultRedirectURI"].as_str().map(String::from);
    let token_validity_units = if input["TokenValidityUnits"].is_object() {
        Some(input["TokenValidityUnits"].clone())
    } else {
        None
    };

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    validate_client_attributes(&pool, &read_attributes, &write_attributes)?;
    validate_client_oauth(
        &pool,
        &callback_urls,
        &logout_urls,
        &allowed_oauth_scopes,
        &supported_identity_providers,
    )?;

    let client_id = random_alnum(26);
    let now = now_epoch();

    let client = UserPoolClient {
        client_id: client_id.clone(),
        client_name: client_name.to_string(),
        user_pool_id: pool_id.to_string(),
        explicit_auth_flows,
        created_date: now,
        client_secret: client_secret.clone(),
        callback_urls,
        logout_urls,
        allowed_oauth_flows,
        allowed_oauth_scopes,
        supported_identity_providers,
        access_token_validity,
        id_token_validity,
        refresh_token_validity,
        additional_client_secrets: Vec::new(),
        read_attributes,
        write_attributes,
        prevent_user_existence_errors,
        enable_token_revocation,
        auth_session_validity,
        allowed_oauth_flows_user_pool_client,
        default_redirect_uri,
        token_validity_units,
        last_modified_date: now,
    };

    let response = client_to_value(&client);
    pool.clients.insert(client_id.clone(), client);

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: created user pool client");

    Ok(json!({ "UserPoolClient": response }))
}

// ---------------------------------------------------------------------------
// DescribeUserPoolClient
// ---------------------------------------------------------------------------

pub fn describe_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

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

    Ok(json!({ "UserPoolClient": client_to_value(client) }))
}

// ---------------------------------------------------------------------------
// DeleteUserPoolClient
// ---------------------------------------------------------------------------

pub fn delete_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    if pool.clients.remove(client_id).is_none() {
        return Err(AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        ));
    }

    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: deleted user pool client");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListUserPoolClients
// ---------------------------------------------------------------------------

pub fn list_user_pool_clients(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    use awsim_core::pagination::{cap_max_results, paginate};

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let mut clients: Vec<(String, String, String)> = pool
        .clients
        .values()
        .map(|c| {
            (
                c.client_id.clone(),
                c.client_name.clone(),
                c.user_pool_id.clone(),
            )
        })
        .collect();
    clients.sort_by(|a, b| a.0.cmp(&b.0));

    let limit = cap_max_results(input["MaxResults"].as_i64(), 60, 60);
    let token = input["NextToken"].as_str();
    let page = paginate(clients, limit, token, |c| c.0.clone())?;

    let client_values: Vec<Value> = page
        .items
        .iter()
        .map(|(id, name, pool_id)| {
            json!({ "ClientId": id, "ClientName": name, "UserPoolId": pool_id })
        })
        .collect();

    let mut resp = json!({ "UserPoolClients": client_values });
    if let Some(next) = page.next_token {
        resp["NextToken"] = json!(next);
    }
    Ok(resp)
}

// ---------------------------------------------------------------------------
// UpdateUserPoolClient
// ---------------------------------------------------------------------------

pub fn update_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let client_id = input["ClientId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "ClientId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    // Parse + validate Read/WriteAttributes before taking the mutable
    // client borrow (validation needs `&pool.schema`). Each list is
    // validated independently; an absent key leaves it unchanged.
    let read_update: Option<Vec<String>> = input["ReadAttributes"].as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });
    let write_update: Option<Vec<String>> = input["WriteAttributes"].as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });
    if read_update.is_some() || write_update.is_some() {
        validate_client_attributes(
            &pool,
            read_update.as_deref().unwrap_or(&[]),
            write_update.as_deref().unwrap_or(&[]),
        )?;
    }

    // Validate any OAuth-related fields present in the update against the pool.
    let parse_list = |key: &str| -> Vec<String> {
        input[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    validate_client_oauth(
        &pool,
        &parse_list("CallbackURLs"),
        &parse_list("LogoutURLs"),
        &parse_list("AllowedOAuthScopes"),
        &parse_list("SupportedIdentityProviders"),
    )?;

    let client = pool.clients.get_mut(client_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool client {client_id} does not exist."),
        )
    })?;

    if let Some(name) = input["ClientName"].as_str() {
        client.client_name = name.to_string();
    }

    if let Some(flows) = input["ExplicitAuthFlows"].as_array() {
        client.explicit_auth_flows = flows
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(urls) = input["CallbackURLs"].as_array() {
        client.callback_urls = urls
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(urls) = input["LogoutURLs"].as_array() {
        client.logout_urls = urls
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(flows) = input["AllowedOAuthFlows"].as_array() {
        client.allowed_oauth_flows = flows
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(scopes) = input["AllowedOAuthScopes"].as_array() {
        client.allowed_oauth_scopes = scopes
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(idps) = input["SupportedIdentityProviders"].as_array() {
        client.supported_identity_providers = idps
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    if let Some(v) = input["AccessTokenValidity"].as_u64() {
        client.access_token_validity = v;
    }
    if let Some(v) = input["IdTokenValidity"].as_u64() {
        client.id_token_validity = v;
    }
    if let Some(v) = input["RefreshTokenValidity"].as_u64() {
        client.refresh_token_validity = v;
    }

    if let Some(read) = read_update {
        client.read_attributes = read;
    }
    if let Some(write) = write_update {
        client.write_attributes = write;
    }

    if let Some(v) = input["PreventUserExistenceErrors"].as_str() {
        client.prevent_user_existence_errors = v.to_string();
    }
    if let Some(v) = input["EnableTokenRevocation"].as_bool() {
        client.enable_token_revocation = v;
    }
    if let Some(v) = input["AuthSessionValidity"].as_u64() {
        client.auth_session_validity = v as u32;
    }
    if let Some(v) = input["AllowedOAuthFlowsUserPoolClient"].as_bool() {
        client.allowed_oauth_flows_user_pool_client = v;
    }
    if let Some(v) = input["DefaultRedirectURI"].as_str() {
        client.default_redirect_uri = Some(v.to_string());
    }
    if input["TokenValidityUnits"].is_object() {
        client.token_validity_units = Some(input["TokenValidityUnits"].clone());
    }
    client.last_modified_date = now_epoch();

    let client_value = client_to_value(client);
    info!(pool_id = %pool_id, client_id = %client_id, "Cognito: updated user pool client");

    Ok(json!({ "UserPoolClient": client_value }))
}

// ---------------------------------------------------------------------------
// AddCustomAttributes
// ---------------------------------------------------------------------------

pub fn add_custom_attributes(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let Some(attrs) = input["CustomAttributes"].as_array() else {
        return Ok(json!({}));
    };

    let mut parsed: Vec<SchemaAttribute> = Vec::with_capacity(attrs.len());
    for attr in attrs {
        let name = attr["Name"].as_str().ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "Attribute Name is required")
        })?;
        parsed.push(parse_custom_schema_entry(name, attr)?);
    }

    let existing_custom = pool
        .schema
        .iter()
        .filter(|a| a.name.starts_with("custom:"))
        .count();
    if existing_custom + parsed.len() > MAX_CUSTOM_ATTRIBUTES {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "User pool already has {existing_custom} custom attributes; \
                 adding {n} would exceed the limit of {MAX_CUSTOM_ATTRIBUTES}",
                n = parsed.len()
            ),
        ));
    }

    for attr in &parsed {
        if pool.schema.iter().any(|a| a.name == attr.name) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("custom attribute {} already exists.", attr.name),
            ));
        }
    }

    pool.schema.extend(parsed);

    info!(pool_id = %pool_id, "Cognito: added custom attributes");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the initial schema for a new user pool: the AWS-defined
/// 20 standard OIDC attributes, plus any entries supplied via the
/// `Schema` parameter on `CreateUserPool`.
///
/// `Schema` entries with a `custom:`-prefixed name (or one without
/// a prefix that doesn't collide with a standard attr) are added
/// as new custom attributes. Entries whose name matches an existing
/// standard attribute override that attribute's `Required` /
/// `Mutable` / `DeveloperOnlyAttribute` flags - real Cognito uses
/// this to let pool creators say e.g. "email is required at signup".
fn build_initial_schema(input: &Value) -> Result<Vec<SchemaAttribute>, AwsError> {
    let mut schema = default_user_pool_schema();
    let Some(arr) = input.as_array() else {
        return Ok(schema);
    };

    for entry in arr {
        let name = entry["Name"].as_str().ok_or_else(|| {
            AwsError::bad_request("InvalidParameterException", "Schema entry missing Name")
        })?;

        if let Some(existing) = schema.iter_mut().find(|a| a.name == name) {
            if let Some(req) = entry["Required"].as_bool() {
                existing.required = req;
            }
            if let Some(mut_) = entry["Mutable"].as_bool() {
                existing.mutable = mut_;
            }
            if let Some(dev) = entry["DeveloperOnlyAttribute"].as_bool() {
                existing.developer_only_attribute = dev;
            }
            if let Some(t) = entry["AttributeDataType"].as_str() {
                validate_data_type(t)?;
            }
            apply_constraints(existing, entry);
            continue;
        }

        let parsed = parse_custom_schema_entry(name, entry)?;
        if schema.iter().any(|a| a.name == parsed.name) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Schema attribute {} declared more than once.", parsed.name),
            ));
        }
        schema.push(parsed);
    }

    let custom_count = schema
        .iter()
        .filter(|a| a.name.starts_with("custom:"))
        .count();
    if custom_count > MAX_CUSTOM_ATTRIBUTES {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Schema declares {custom_count} custom attributes, exceeding the limit of \
                 {MAX_CUSTOM_ATTRIBUTES}"
            ),
        ));
    }

    Ok(schema)
}

/// Parse a single `Schema` / `CustomAttributes` entry, resolving the
/// `custom:` prefix. Used by both `CreateUserPool` (for non-standard
/// names) and `AddCustomAttributes`.
fn parse_custom_schema_entry(name: &str, entry: &Value) -> Result<SchemaAttribute, AwsError> {
    if name.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Schema attribute Name is empty",
        ));
    }
    let base_name = name.strip_prefix("custom:").unwrap_or(name);
    let full_name = if name.starts_with("custom:") {
        name.to_string()
    } else {
        format!("custom:{name}")
    };

    // Custom attribute names are 1-20 characters (excluding the prefix).
    let base_len = base_name.chars().count();
    if !(1..=20).contains(&base_len) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Custom attribute name must be between 1 and 20 characters.",
        ));
    }

    // Cognito does not allow custom attributes to be Required.
    if entry["Required"].as_bool() == Some(true) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Required custom attributes are not supported currently.",
        ));
    }

    let data_type = entry["AttributeDataType"].as_str().unwrap_or("String");
    validate_data_type(data_type)?;

    let mut attr = SchemaAttribute {
        name: full_name,
        attribute_data_type: data_type.to_string(),
        required: entry["Required"].as_bool().unwrap_or(false),
        mutable: entry["Mutable"].as_bool().unwrap_or(true),
        developer_only_attribute: entry["DeveloperOnlyAttribute"].as_bool().unwrap_or(false),
        string_attribute_constraints: None,
        number_attribute_constraints: None,
    };
    apply_constraints(&mut attr, entry);
    Ok(attr)
}

fn validate_data_type(t: &str) -> Result<(), AwsError> {
    match t {
        "String" | "Number" | "DateTime" | "Boolean" => Ok(()),
        other => Err(AwsError::bad_request(
            "InvalidParameterException",
            format!("Unknown AttributeDataType: {other}"),
        )),
    }
}

/// Mirror AWS's wire shape: `MinLength` / `MaxLength` / `MinValue` /
/// `MaxValue` are string-encoded. Tolerate both string and numeric
/// JSON for resilience against SDKs that auto-coerce.
fn apply_constraints(attr: &mut SchemaAttribute, entry: &Value) {
    if attr.attribute_data_type == "String" {
        let c = &entry["StringAttributeConstraints"];
        let min = parse_string_or_number(&c["MinLength"]);
        let max = parse_string_or_number(&c["MaxLength"]);
        if min.is_some() || max.is_some() {
            attr.string_attribute_constraints = Some(StringAttributeConstraints {
                min_length: min.map(|v| v.max(0) as u32),
                max_length: max.map(|v| v.max(0) as u32),
            });
        }
    }
    if attr.attribute_data_type == "Number" {
        let c = &entry["NumberAttributeConstraints"];
        let min = parse_string_or_number(&c["MinValue"]);
        let max = parse_string_or_number(&c["MaxValue"]);
        if min.is_some() || max.is_some() {
            attr.number_attribute_constraints = Some(NumberAttributeConstraints {
                min_value: min,
                max_value: max,
            });
        }
    }
}

fn parse_string_or_number(v: &Value) -> Option<i64> {
    if let Some(s) = v.as_str() {
        s.parse().ok()
    } else {
        v.as_i64()
    }
}

/// Render a `SchemaAttribute` in the `SchemaAttributeType` shape
/// `DescribeUserPool` returns - constraints come back as decimal
/// strings to match AWS.
pub fn schema_attr_to_value(attr: &SchemaAttribute) -> Value {
    let mut obj = json!({
        "Name": attr.name,
        "AttributeDataType": attr.attribute_data_type,
        "DeveloperOnlyAttribute": attr.developer_only_attribute,
        "Mutable": attr.mutable,
        "Required": attr.required,
    });
    if let Some(c) = &attr.string_attribute_constraints {
        let mut sc = serde_json::Map::new();
        if let Some(min) = c.min_length {
            sc.insert("MinLength".into(), Value::String(min.to_string()));
        }
        if let Some(max) = c.max_length {
            sc.insert("MaxLength".into(), Value::String(max.to_string()));
        }
        obj.as_object_mut()
            .expect("json!() macro produces object")
            .insert("StringAttributeConstraints".into(), Value::Object(sc));
    }
    if let Some(c) = &attr.number_attribute_constraints {
        let mut nc = serde_json::Map::new();
        if let Some(min) = c.min_value {
            nc.insert("MinValue".into(), Value::String(min.to_string()));
        }
        if let Some(max) = c.max_value {
            nc.insert("MaxValue".into(), Value::String(max.to_string()));
        }
        obj.as_object_mut()
            .expect("json!() macro produces object")
            .insert("NumberAttributeConstraints".into(), Value::Object(nc));
    }
    obj
}

/// `Policies.SignInPolicy.AllowedFirstAuthFactors` must be a non-empty
/// subset of the documented enum when supplied. Returns the parsed
/// list (empty when the caller omitted the policy entirely so default
/// PASSWORD-only behavior still applies).
fn parse_sign_in_policy(v: &Value) -> Result<Vec<String>, AwsError> {
    if v.is_null() {
        return Ok(Vec::new());
    }
    let obj = v.as_object().ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameterException",
            "Policies.SignInPolicy must be an object.",
        )
    })?;
    let Some(arr) = obj.get("AllowedFirstAuthFactors").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if arr.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            "Policies.SignInPolicy.AllowedFirstAuthFactors must be non-empty when supplied.",
        ));
    }
    let mut out = Vec::with_capacity(arr.len());
    let mut seen = std::collections::HashSet::new();
    for entry in arr {
        let f = entry.as_str().ok_or_else(|| {
            AwsError::bad_request(
                "InvalidParameterException",
                "Policies.SignInPolicy.AllowedFirstAuthFactors entries must be strings.",
            )
        })?;
        if !matches!(f, "PASSWORD" | "WEB_AUTHN" | "EMAIL_OTP" | "SMS_OTP") {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!(
                    "Policies.SignInPolicy.AllowedFirstAuthFactors `{f}` must be one of \
                     PASSWORD, WEB_AUTHN, EMAIL_OTP, SMS_OTP."
                ),
            ));
        }
        if !seen.insert(f.to_string()) {
            return Err(AwsError::bad_request(
                "InvalidParameterException",
                format!("Policies.SignInPolicy.AllowedFirstAuthFactors contains duplicate `{f}`."),
            ));
        }
        out.push(f.to_string());
    }
    Ok(out)
}

fn parse_password_policy(v: &Value) -> Result<PasswordPolicy, AwsError> {
    let default = PasswordPolicy::default();
    let minimum_length = v["MinimumLength"]
        .as_u64()
        .unwrap_or(default.minimum_length as u64);
    if !(6..=99).contains(&minimum_length) {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Policies.PasswordPolicy.MinimumLength must be between 6 and 99 (got {minimum_length})."
            ),
        ));
    }
    let temporary_password_validity_days = v["TemporaryPasswordValidityDays"]
        .as_u64()
        .unwrap_or(default.temporary_password_validity_days as u64);
    if temporary_password_validity_days > 365 {
        return Err(AwsError::bad_request(
            "InvalidParameterException",
            format!(
                "Policies.PasswordPolicy.TemporaryPasswordValidityDays must be at most 365 (got {temporary_password_validity_days})."
            ),
        ));
    }
    Ok(PasswordPolicy {
        minimum_length: minimum_length as u32,
        require_lowercase: v["RequireLowercase"]
            .as_bool()
            .unwrap_or(default.require_lowercase),
        require_uppercase: v["RequireUppercase"]
            .as_bool()
            .unwrap_or(default.require_uppercase),
        require_numbers: v["RequireNumbers"]
            .as_bool()
            .unwrap_or(default.require_numbers),
        require_symbols: v["RequireSymbols"]
            .as_bool()
            .unwrap_or(default.require_symbols),
        temporary_password_validity_days: temporary_password_validity_days as u32,
    })
}

fn parse_lambda_config(v: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(s) = val.as_str() {
                // Plain trigger -> ARN (PreSignUp, PostConfirmation, the V1
                // PreTokenGeneration, ...).
                map.insert(k.clone(), s.to_string());
            } else if let Some(inner) = val.as_object() {
                // Nested trigger config (PreTokenGenerationConfig,
                // CustomEmailSender, CustomSMSSender) carries LambdaArn +
                // LambdaVersion. Previously these were dropped entirely.
                // Flatten the ARN under the trigger key minus the "Config"
                // suffix and keep the version under "<key>Version" so the
                // trigger can still be resolved and versioned.
                if let Some(arn) = inner.get("LambdaArn").and_then(|a| a.as_str()) {
                    let base = k.strip_suffix("Config").unwrap_or(k);
                    map.insert(base.to_string(), arn.to_string());
                    if let Some(ver) = inner.get("LambdaVersion").and_then(|x| x.as_str()) {
                        map.insert(format!("{base}Version"), ver.to_string());
                    }
                }
            }
        }
    }
    map
}

/// Parse a `{key: value}` string map (UserPoolTags).
fn parse_tag_map(v: &Value) -> HashMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Pool configuration blocks awsim stores and echoes verbatim. Keep this in
/// sync with the doc on `UserPool::extra_config`.
const EXTRA_POOL_CONFIG_KEYS: &[&str] = &[
    "AdminCreateUserConfig",
    "EmailConfiguration",
    "SmsConfiguration",
    "DeviceConfiguration",
    "UsernameConfiguration",
    "AccountRecoverySetting",
    "VerificationMessageTemplate",
    "DeletionProtection",
    "UserPoolAddOns",
    "EmailVerificationMessage",
    "EmailVerificationSubject",
    "SmsVerificationMessage",
    "SmsAuthenticationMessage",
    "UserAttributeUpdateSettings",
];

/// Capture the verbatim config blocks present in a Create/Update request,
/// filling in the AWS defaults AWS reports for an unset pool.
fn parse_extra_pool_config(input: &Value) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for key in EXTRA_POOL_CONFIG_KEYS {
        if !input[*key].is_null() {
            map.insert((*key).to_string(), input[*key].clone());
        }
    }
    map.entry("DeletionProtection".to_string())
        .or_insert_with(|| Value::String("INACTIVE".to_string()));
    map.entry("AdminCreateUserConfig".to_string())
        .or_insert_with(
            || json!({ "AllowAdminCreateUserOnly": false, "UnusedAccountValidityDays": 7 }),
        );
    map.entry("EmailConfiguration".to_string())
        .or_insert_with(|| json!({ "EmailSendingAccount": "COGNITO_DEFAULT" }));
    map
}

fn parse_string_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// GetSigningCertificate
// ---------------------------------------------------------------------------

pub fn get_signing_certificate(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let _pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    // Return a stub PEM certificate
    let fake_pem = "-----BEGIN CERTIFICATE-----\n\
        MIICpDCCAYwCCQDU+pQ4pHgSpDANBgkqhkiG9w0BAQsFADAUMRIwEAYDVQQDDAls\n\
        b2NhbGhvc3QwHhcNMjQwMTAxMDAwMDAwWhcNMjUwMTAxMDAwMDAwWjAUMRIwEAYD\n\
        VQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQC7\n\
        o4qne60TB3pNjMEm9+MnEL4skPmNgBsixiPEOThqxhV2IVNkMcPGEMaFOfFsaXHf\n\
        awsim-fake-cognito-signing-certificate-for-local-development-only\n\
        -----END CERTIFICATE-----";

    Ok(json!({ "Certificate": fake_pem }))
}

// ---------------------------------------------------------------------------
// GetLogDeliveryConfiguration
// ---------------------------------------------------------------------------

pub fn get_log_delivery_configuration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    let log_configs: Vec<serde_json::Value> = pool
        .log_delivery_configuration
        .as_ref()
        .map(|ldc| ldc.log_configurations.clone())
        .unwrap_or_default();

    Ok(json!({
        "LogDeliveryConfiguration": {
            "UserPoolId": pool_id,
            "LogConfigurations": log_configs
        }
    }))
}

// ---------------------------------------------------------------------------
// SetLogDeliveryConfiguration
// ---------------------------------------------------------------------------

pub fn set_log_delivery_configuration(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    use crate::state::LogDeliveryConfiguration;

    let pool_id = input["UserPoolId"].as_str().ok_or_else(|| {
        AwsError::bad_request("InvalidParameterException", "UserPoolId is required")
    })?;
    let log_configs: Vec<serde_json::Value> = input["LogConfigurations"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("User pool {pool_id} does not exist."),
        )
    })?;

    pool.log_delivery_configuration = Some(LogDeliveryConfiguration {
        log_configurations: log_configs.clone(),
    });

    info!(pool_id = %pool_id, "Cognito: set log delivery configuration");
    Ok(json!({
        "LogDeliveryConfiguration": {
            "UserPoolId": pool_id,
            "LogConfigurations": log_configs
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("cognito-idp", "us-east-1")
    }

    #[test]
    fn create_user_pool_seeds_default_schema() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool = state
            .user_pools
            .iter()
            .next()
            .expect("pool was created")
            .clone();
        // 20 standard OIDC attrs out of the box.
        assert_eq!(pool.schema.len(), 20);
        assert!(pool.schema.iter().any(|a| a.name == "email"));
        assert!(pool.schema.iter().any(|a| a.name == "sub"));
        assert!(pool.schema.iter().any(|a| a.name == "updated_at"));
    }

    #[test]
    fn create_user_pool_with_custom_schema_attr_adds_to_schema() {
        let state = CognitoState::default();
        let input = json!({
            "PoolName": "p",
            "Schema": [
                {
                    "Name": "plan",
                    "AttributeDataType": "String",
                    "Mutable": true,
                    "StringAttributeConstraints": { "MinLength": "1", "MaxLength": "32" }
                },
                {
                    "Name": "rank",
                    "AttributeDataType": "Number",
                    "NumberAttributeConstraints": { "MinValue": "0", "MaxValue": "10" }
                }
            ]
        });
        create_user_pool(&state, &input, &ctx()).unwrap();
        let pool = state
            .user_pools
            .iter()
            .next()
            .expect("pool was created")
            .clone();
        let plan = pool
            .schema
            .iter()
            .find(|a| a.name == "custom:plan")
            .expect("custom:plan in schema");
        assert_eq!(plan.attribute_data_type, "String");
        assert_eq!(
            plan.string_attribute_constraints
                .as_ref()
                .unwrap()
                .max_length,
            Some(32)
        );
        let rank = pool
            .schema
            .iter()
            .find(|a| a.name == "custom:rank")
            .expect("custom:rank in schema");
        assert_eq!(
            rank.number_attribute_constraints
                .as_ref()
                .unwrap()
                .max_value,
            Some(10)
        );
    }

    #[test]
    fn create_user_pool_schema_can_override_standard_attr_required_flag() {
        let state = CognitoState::default();
        let input = json!({
            "PoolName": "p",
            "Schema": [{ "Name": "email", "Required": true }]
        });
        create_user_pool(&state, &input, &ctx()).unwrap();
        let pool = state
            .user_pools
            .iter()
            .next()
            .expect("pool was created")
            .clone();
        let email = pool.schema.iter().find(|a| a.name == "email").unwrap();
        assert!(email.required);
        // Schema length doesn't grow - the entry overrode rather than appended.
        assert_eq!(pool.schema.len(), 20);
    }

    #[test]
    fn create_user_pool_rejects_unknown_data_type() {
        let state = CognitoState::default();
        let input = json!({
            "PoolName": "p",
            "Schema": [{ "Name": "weird", "AttributeDataType": "Hex" }]
        });
        let err = create_user_pool(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn add_custom_attributes_rejects_duplicate() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();

        let input = json!({
            "UserPoolId": pool_id,
            "CustomAttributes": [{ "Name": "plan", "AttributeDataType": "String" }]
        });
        add_custom_attributes(&state, &input, &ctx()).unwrap();
        let err = add_custom_attributes(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("already exists"));
    }

    #[test]
    fn add_custom_attributes_enforces_50_cap() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();

        let attrs: Vec<Value> = (0..50)
            .map(|i| json!({ "Name": format!("a{i}"), "AttributeDataType": "String" }))
            .collect();
        add_custom_attributes(
            &state,
            &json!({ "UserPoolId": pool_id, "CustomAttributes": attrs }),
            &ctx(),
        )
        .unwrap();

        let err = add_custom_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "CustomAttributes": [{ "Name": "overflow", "AttributeDataType": "String" }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("limit"));
    }

    #[test]
    fn add_custom_attributes_rejects_unknown_data_type() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();
        let err = add_custom_attributes(
            &state,
            &json!({
                "UserPoolId": pool_id,
                "CustomAttributes": [{ "Name": "weird", "AttributeDataType": "Hex" }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_user_pool_accepts_sign_in_policy() {
        let state = CognitoState::default();
        let resp = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Policies": {
                    "SignInPolicy": {
                        "AllowedFirstAuthFactors": ["PASSWORD", "EMAIL_OTP"]
                    }
                }
            }),
            &ctx(),
        )
        .unwrap();
        let pool_id = resp["UserPool"]["Id"].as_str().unwrap().to_string();
        let desc = describe_user_pool(&state, &json!({ "UserPoolId": pool_id }), &ctx()).unwrap();
        let factors = desc["UserPool"]["Policies"]["SignInPolicy"]["AllowedFirstAuthFactors"]
            .as_array()
            .expect("AllowedFirstAuthFactors populated");
        assert_eq!(factors.len(), 2);
    }

    #[test]
    fn create_user_pool_rejects_unknown_first_auth_factor() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Policies": {
                    "SignInPolicy": {
                        "AllowedFirstAuthFactors": ["MAGIC"]
                    }
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_user_pool_rejects_empty_first_auth_factors() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Policies": {
                    "SignInPolicy": {
                        "AllowedFirstAuthFactors": []
                    }
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn create_user_pool_rejects_duplicate_first_auth_factor() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Policies": {
                    "SignInPolicy": {
                        "AllowedFirstAuthFactors": ["PASSWORD", "PASSWORD"]
                    }
                }
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn describe_user_pool_returns_schema_attributes() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state
            .user_pools
            .iter()
            .next()
            .expect("pool created")
            .id
            .clone();
        let resp = describe_user_pool(&state, &json!({ "UserPoolId": pool_id }), &ctx()).unwrap();
        let attrs = resp["UserPool"]["SchemaAttributes"]
            .as_array()
            .expect("SchemaAttributes is an array");
        assert_eq!(attrs.len(), 20);
        let email = attrs
            .iter()
            .find(|a| a["Name"] == "email")
            .expect("email in response");
        assert_eq!(email["AttributeDataType"], "String");
        assert!(email["StringAttributeConstraints"]["MaxLength"].is_string());
    }

    #[test]
    fn parse_lambda_config_keeps_nested_pretoken_config() {
        // Plain V1 trigger plus the nested V2 PreTokenGenerationConfig that
        // was previously dropped on the floor.
        let cfg = parse_lambda_config(&json!({
            "PreSignUp": "arn:fn:presignup",
            "PreTokenGenerationConfig": {
                "LambdaArn": "arn:fn:pretoken",
                "LambdaVersion": "V2_0"
            }
        }));
        assert_eq!(
            cfg.get("PreSignUp").map(String::as_str),
            Some("arn:fn:presignup")
        );
        assert_eq!(
            cfg.get("PreTokenGeneration").map(String::as_str),
            Some("arn:fn:pretoken")
        );
        assert_eq!(
            cfg.get("PreTokenGenerationVersion").map(String::as_str),
            Some("V2_0")
        );
    }

    #[test]
    fn create_user_pool_round_trips_config_and_tags() {
        let state = CognitoState::default();
        let created = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "UserPoolTags": { "team": "auth" },
                "DeletionProtection": "ACTIVE",
                "AdminCreateUserConfig": { "AllowAdminCreateUserOnly": true },
                "LambdaConfig": { "PreSignUp": "arn:fn:presignup" }
            }),
            &ctx(),
        )
        .unwrap();
        // CreateUserPool returns the full pool object, not a stub.
        assert_eq!(created["UserPool"]["UserPoolTags"]["team"], "auth");
        assert_eq!(created["UserPool"]["DeletionProtection"], "ACTIVE");
        assert_eq!(
            created["UserPool"]["LambdaConfig"]["PreSignUp"],
            "arn:fn:presignup"
        );
        let pool_id = created["UserPool"]["Id"].as_str().unwrap().to_string();
        let resp = describe_user_pool(&state, &json!({ "UserPoolId": pool_id }), &ctx()).unwrap();
        assert_eq!(resp["UserPool"]["UserPoolTags"]["team"], "auth");
        assert_eq!(resp["UserPool"]["DeletionProtection"], "ACTIVE");
        assert_eq!(
            resp["UserPool"]["AdminCreateUserConfig"]["AllowAdminCreateUserOnly"],
            true
        );
    }

    #[test]
    fn list_user_pools_paginates() {
        let state = CognitoState::default();
        for i in 0..3 {
            create_user_pool(&state, &json!({ "PoolName": format!("p{i}") }), &ctx()).unwrap();
        }
        let first = list_user_pools(&state, &json!({ "MaxResults": 2 }), &ctx()).unwrap();
        assert_eq!(first["UserPools"].as_array().unwrap().len(), 2);
        let next = first["NextToken"].as_str().expect("more pages");
        let second = list_user_pools(
            &state,
            &json!({ "MaxResults": 2, "NextToken": next }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(second["UserPools"].as_array().unwrap().len(), 1);
        assert!(second["NextToken"].is_null());
    }

    #[test]
    fn client_describe_returns_secret_and_config_defaults() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state.user_pools.iter().next().unwrap().id.clone();
        let created = create_user_pool_client(
            &state,
            &json!({ "UserPoolId": pool_id, "ClientName": "c", "GenerateSecret": true }),
            &ctx(),
        )
        .unwrap();
        let client_id = created["UserPoolClient"]["ClientId"]
            .as_str()
            .unwrap()
            .to_string();
        // Defaults match AWS, and an unset ExplicitAuthFlows expands to the
        // SRP/custom/refresh set.
        assert_eq!(
            created["UserPoolClient"]["PreventUserExistenceErrors"],
            "LEGACY"
        );
        assert_eq!(created["UserPoolClient"]["EnableTokenRevocation"], true);
        assert_eq!(created["UserPoolClient"]["AuthSessionValidity"], 3);
        let flows: Vec<String> = created["UserPoolClient"]["ExplicitAuthFlows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(flows.contains(&"ALLOW_USER_SRP_AUTH".to_string()));
        assert!(flows.contains(&"ALLOW_REFRESH_TOKEN_AUTH".to_string()));
        // DescribeUserPoolClient returns the ClientSecret (not just Create).
        let described = describe_user_pool_client(
            &state,
            &json!({ "UserPoolId": pool_id, "ClientId": client_id }),
            &ctx(),
        )
        .unwrap();
        assert!(described["UserPoolClient"]["ClientSecret"].is_string());
    }

    #[test]
    fn create_client_rejects_unknown_idp_and_bad_callback_and_scope() {
        let state = CognitoState::default();
        create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = state.user_pools.iter().next().unwrap().id.clone();
        let mk = |body: Value| create_user_pool_client(&state, &body, &ctx());

        let err = mk(json!({ "UserPoolId": pool_id, "ClientName": "c",
                             "SupportedIdentityProviders": ["Nope"] }))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");

        let err = mk(json!({ "UserPoolId": pool_id, "ClientName": "c",
                             "CallbackURLs": ["http://evil.example.com/cb"] }))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");

        let err = mk(json!({ "UserPoolId": pool_id, "ClientName": "c",
                             "AllowedOAuthScopes": ["my-rs/read"] }))
        .unwrap_err();
        assert_eq!(err.code, "ScopeDoesNotExistException");

        // A built-in IdP, https callback, and standard scope are accepted.
        mk(json!({ "UserPoolId": pool_id, "ClientName": "c",
                   "SupportedIdentityProviders": ["COGNITO"],
                   "CallbackURLs": ["https://app.example.com/cb", "http://localhost:3000/cb"],
                   "AllowedOAuthScopes": ["openid", "email"] }))
        .unwrap();
    }

    #[test]
    fn create_user_pool_rejects_required_custom_attribute() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Schema": [{ "Name": "org", "AttributeDataType": "String", "Required": true }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
        assert!(err.message.contains("Required custom attributes"));
    }

    #[test]
    fn create_user_pool_rejects_overlong_custom_attribute_name() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "Schema": [{ "Name": "this_name_is_far_too_long", "AttributeDataType": "String" }]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn pool_and_client_ids_are_alphanumeric() {
        let state = CognitoState::default();
        let created = create_user_pool(&state, &json!({ "PoolName": "p" }), &ctx()).unwrap();
        let pool_id = created["UserPool"]["Id"].as_str().unwrap().to_string();
        let suffix = pool_id.split_once('_').unwrap().1;
        assert_eq!(suffix.len(), 9);
        assert!(suffix.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(
            created["UserPool"]["EmailConfiguration"]["EmailSendingAccount"],
            "COGNITO_DEFAULT"
        );
        let client = create_user_pool_client(
            &state,
            &json!({ "UserPoolId": pool_id, "ClientName": "c" }),
            &ctx(),
        )
        .unwrap();
        let cid = client["UserPoolClient"]["ClientId"].as_str().unwrap();
        assert_eq!(cid.len(), 26);
        assert!(cid.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn create_user_pool_rejects_username_and_alias_attributes_together() {
        let state = CognitoState::default();
        let err = create_user_pool(
            &state,
            &json!({
                "PoolName": "p",
                "UsernameAttributes": ["email"],
                "AliasAttributes": ["phone_number"]
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }
}
