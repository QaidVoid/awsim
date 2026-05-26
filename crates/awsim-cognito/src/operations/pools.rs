use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use rand::Rng;
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

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

fn pool_arn(region: &str, account_id: &str, pool_id: &str) -> String {
    format!("arn:aws:cognito-idp:{region}:{account_id}:userpool/{pool_id}")
}

/// Generate a random 52-char alphanumeric client secret.
fn generate_client_secret() -> String {
    let mut rng = rand::thread_rng();
    (0..52)
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

fn client_to_value(client: &UserPoolClient, include_secret: bool) -> Value {
    let mut obj = json!({
        "UserPoolId": client.user_pool_id,
        "ClientName": client.client_name,
        "ClientId": client.client_id,
        "ExplicitAuthFlows": client.explicit_auth_flows,
        "CallbackURLs": client.callback_urls,
        "LogoutURLs": client.logout_urls,
        "AllowedOAuthFlows": client.allowed_oauth_flows,
        "AllowedOAuthScopes": client.allowed_oauth_scopes,
        "SupportedIdentityProviders": client.supported_identity_providers,
        "AccessTokenValidity": client.access_token_validity,
        "IdTokenValidity": client.id_token_validity,
        "RefreshTokenValidity": client.refresh_token_validity,
        "CreationDate": client.created_date,
        "LastModifiedDate": client.created_date
    });
    // SAFETY: obj was created by json!() macro above, which always produces an object.
    let map = obj
        .as_object_mut()
        .expect("json!() macro always produces an object");
    if include_secret {
        map.insert("ClientSecret".into(), json!(client.client_secret));
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

// ---------------------------------------------------------------------------
// CreateUserPool
// ---------------------------------------------------------------------------

pub fn create_user_pool(
    state: &CognitoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_name = input["PoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PoolName is required"))?;

    let random = &Uuid::new_v4().to_string()[..8];
    let pool_id = format!("{0}_{1}", ctx.region, random);
    let arn = pool_arn(&ctx.region, &ctx.account_id, &pool_id);
    let now = now_epoch();

    // Parse policies from input
    let policies = parse_password_policy(&input["Policies"]["PasswordPolicy"])?;

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

    let lambda_config = parse_lambda_config(&input["LambdaConfig"]);

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
        tags: HashMap::new(),
        ui_customizations: HashMap::new(),
        managed_login_brandings: Vec::new(),
        risk_configurations: Vec::new(),
        import_jobs: Vec::new(),
        log_delivery_configuration: None,
        terms: Vec::new(),
        custom_auth_expected_answer: None,
        custom_auth_challenge_parameters: HashMap::new(),
    };

    info!(pool_id = %pool_id, "Cognito: created user pool");
    state.user_pools.insert(pool_id.clone(), pool);

    Ok(json!({
        "UserPool": {
            "Id": pool_id,
            "Name": pool_name,
            "Arn": arn,
            "Status": "Active",
            "CreationDate": now,
            "LastModifiedDate": now
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteUserPool
// ---------------------------------------------------------------------------

pub fn delete_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    if state.user_pools.remove(pool_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        ));
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let schema_attributes: Vec<Value> = pool.schema.iter().map(schema_attr_to_value).collect();

    Ok(json!({
        "UserPool": {
            "Id": pool.id,
            "Name": pool.name,
            "Arn": pool.arn,
            "Status": "Active",
            "CreationDate": pool.created_date,
            "LastModifiedDate": pool.created_date,
            "MfaConfiguration": pool.mfa_configuration,
            "AutoVerifiedAttributes": pool.auto_verified_attributes,
            "UsernameAttributes": pool.username_attributes,
            "AliasAttributes": pool.alias_attributes,
            "SchemaAttributes": schema_attributes,
            "Policies": {
                "PasswordPolicy": password_policy_to_value(&pool.policies)
            },
            "Domain": pool.domain,
            "EstimatedNumberOfUsers": pool.users.len()
        }
    }))
}

// ---------------------------------------------------------------------------
// ListUserPools
// ---------------------------------------------------------------------------

pub fn list_user_pools(
    state: &CognitoState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pools: Vec<Value> = state
        .user_pools
        .iter()
        .map(|e| {
            json!({
                "Id": e.id,
                "Name": e.name,
                "Status": "Active",
                "CreationDate": e.created_date,
                "LastModifiedDate": e.created_date
            })
        })
        .collect();

    Ok(json!({ "UserPools": pools }))
}

// ---------------------------------------------------------------------------
// UpdateUserPool
// ---------------------------------------------------------------------------

pub fn update_user_pool(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_name = input["ClientName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientName is required"))?;

    let explicit_auth_flows: Vec<String> = input["ExplicitAuthFlows"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

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

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    validate_client_attributes(&pool, &read_attributes, &write_attributes)?;

    let client_id = Uuid::new_v4().to_string().replace('-', "")[..26].to_string();
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
    };

    let response = client_to_value(&client, true);
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

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

    Ok(json!({ "UserPoolClient": client_to_value(client, false) }))
}

// ---------------------------------------------------------------------------
// DeleteUserPoolClient
// ---------------------------------------------------------------------------

pub fn delete_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    if pool.clients.remove(client_id).is_none() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let clients: Vec<Value> = pool
        .clients
        .values()
        .take(max_results)
        .map(|c| {
            json!({
                "ClientId": c.client_id,
                "ClientName": c.client_name,
                "UserPoolId": c.user_pool_id
            })
        })
        .collect();

    Ok(json!({ "UserPoolClients": clients }))
}

// ---------------------------------------------------------------------------
// UpdateUserPoolClient
// ---------------------------------------------------------------------------

pub fn update_user_pool_client(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let client_id = input["ClientId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ClientId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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

    let client = pool.clients.get_mut(client_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Client not found: {client_id}"),
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

    let client_value = client_to_value(client, false);
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
        )
    })?;

    let Some(attrs) = input["CustomAttributes"].as_array() else {
        return Ok(json!({}));
    };

    let mut parsed: Vec<SchemaAttribute> = Vec::with_capacity(attrs.len());
    for attr in attrs {
        let name = attr["Name"].as_str().ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "Attribute Name is required")
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
    let full_name = if name.starts_with("custom:") {
        name.to_string()
    } else {
        format!("custom:{name}")
    };

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
                map.insert(k.clone(), s.to_string());
            }
        }
    }
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let _pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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

    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let log_configs: Vec<serde_json::Value> = input["LogConfigurations"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("User pool not found: {pool_id}"),
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
}
