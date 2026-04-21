use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{ApiKey, AppSyncState, DataSource, GraphqlApi, Resolver, now_iso};

// ── helpers ─────────────────────────────────────────────────────────────────

fn api_to_json(api: &GraphqlApi) -> Value {
    json!({
        "apiId": api.api_id,
        "name": api.name,
        "arn": api.arn,
        "uris": api.uris,
        "authenticationType": api.authentication_type,
        "schemaStatus": api.schema_status,
        "createdAt": api.created_at,
    })
}

fn key_to_json(k: &ApiKey) -> Value {
    json!({
        "id": k.id,
        "description": k.description,
        "expires": k.expires,
    })
}

fn ds_to_json(ds: &DataSource) -> Value {
    json!({
        "name": ds.name,
        "type": ds.data_source_type,
        "description": ds.description,
    })
}

fn resolver_to_json(r: &Resolver) -> Value {
    json!({
        "typeName": r.type_name,
        "fieldName": r.field_name,
        "dataSourceName": r.data_source_name,
    })
}

// ── GraphQL APIs ─────────────────────────────────────────────────────────────

pub fn create_graphql_api(
    state: &AppSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;

    let auth_type = input["authenticationType"]
        .as_str()
        .unwrap_or("API_KEY")
        .to_string();

    let api_id = Uuid::new_v4().to_string().replace('-', "")[..20].to_string();
    let arn = format!(
        "arn:aws:appsync:{}:{}:apis/{}",
        ctx.region, ctx.account_id, api_id
    );
    let mut uris = HashMap::new();
    uris.insert(
        "GRAPHQL".to_string(),
        format!("http://localhost:4566/appsync/{}/graphql", api_id),
    );

    let api = GraphqlApi {
        api_id: api_id.clone(),
        name: name.to_string(),
        arn,
        uris,
        authentication_type: auth_type,
        schema: None,
        schema_status: "NOT_APPLICABLE".to_string(),
        api_keys: Vec::new(),
        data_sources: Vec::new(),
        resolvers: Vec::new(),
        created_at: now_iso(),
    };

    info!(api_id = %api_id, name = %name, "Created GraphQL API");
    let result = api_to_json(&api);
    state.apis.insert(api_id, api);

    Ok(json!({ "graphqlApi": result }))
}

pub fn get_graphql_api(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    Ok(json!({ "graphqlApi": api_to_json(&*api) }))
}

pub fn list_graphql_apis(state: &AppSyncState) -> Result<Value, AwsError> {
    let apis: Vec<Value> = state.apis.iter().map(|e| api_to_json(e.value())).collect();
    Ok(json!({ "graphqlApis": apis }))
}

pub fn delete_graphql_api(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    if state.apis.remove(api_id).is_none() {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        ));
    }

    info!(api_id = %api_id, "Deleted GraphQL API");
    Ok(json!({}))
}

pub fn update_graphql_api(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    if let Some(name) = input["name"].as_str() {
        api.name = name.to_string();
    }
    if let Some(auth) = input["authenticationType"].as_str() {
        api.authentication_type = auth.to_string();
    }

    Ok(json!({ "graphqlApi": api_to_json(&*api) }))
}

// ── Schema ────────────────────────────────────────────────────────────────────

pub fn start_schema_creation(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let definition = input["definition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "definition is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    api.schema = Some(definition.to_string());
    api.schema_status = "ACTIVE".to_string();

    Ok(json!({ "status": "ACTIVE" }))
}

pub fn get_schema_creation_status(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    Ok(json!({ "status": api.schema_status }))
}

// ── API Keys ──────────────────────────────────────────────────────────────────

pub fn create_api_key(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let key_id = format!("da2-{}", &Uuid::new_v4().to_string().replace('-', "")[..20]);
    let description = input["description"].as_str().map(|s| s.to_string());

    // Default expiry: 7 days from now
    let expires = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        (now + 7 * 86400) as i64
    };

    let key = ApiKey {
        id: key_id.clone(),
        description,
        expires,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let result = key_to_json(&key);
    api.api_keys.push(key);

    info!(api_id = %api_id, key_id = %key_id, "Created API key");
    Ok(json!({ "apiKey": result }))
}

pub fn list_api_keys(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let keys: Vec<Value> = api.api_keys.iter().map(key_to_json).collect();
    Ok(json!({ "apiKeys": keys }))
}

pub fn delete_api_key(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let id = input["id"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "id is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let before = api.api_keys.len();
    api.api_keys.retain(|k| k.id != id);
    if api.api_keys.len() == before {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("API key {} not found", id),
        ));
    }

    Ok(json!({}))
}

// ── Data Sources ──────────────────────────────────────────────────────────────

pub fn create_data_source(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;
    let ds_type = input["type"].as_str().unwrap_or("NONE").to_string();
    let description = input["description"].as_str().map(|s| s.to_string());

    let ds = DataSource {
        name: name.to_string(),
        data_source_type: ds_type,
        description,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let result = ds_to_json(&ds);
    api.data_sources.push(ds);

    info!(api_id = %api_id, name = %name, "Created data source");
    Ok(json!({ "dataSource": result }))
}

pub fn list_data_sources(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let sources: Vec<Value> = api.data_sources.iter().map(ds_to_json).collect();
    Ok(json!({ "dataSources": sources }))
}

pub fn delete_data_source(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let before = api.data_sources.len();
    api.data_sources.retain(|d| d.name != name);
    if api.data_sources.len() == before {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Data source {} not found", name),
        ));
    }

    Ok(json!({}))
}

// ── Resolvers ─────────────────────────────────────────────────────────────────

pub fn create_resolver(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;
    let field_name = input["fieldName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "fieldName is required"))?;
    let data_source_name = input["dataSourceName"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let resolver = Resolver {
        type_name: type_name.to_string(),
        field_name: field_name.to_string(),
        data_source_name,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let result = resolver_to_json(&resolver);
    api.resolvers.push(resolver);

    info!(api_id = %api_id, type_name = %type_name, field_name = %field_name, "Created resolver");
    Ok(json!({ "resolver": result }))
}

pub fn list_resolvers(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found("NotFoundException", format!("GraphQL API {} not found", api_id))
    })?;

    let resolvers: Vec<Value> = api
        .resolvers
        .iter()
        .filter(|r| r.type_name == type_name)
        .map(resolver_to_json)
        .collect();

    Ok(json!({ "resolvers": resolvers }))
}
