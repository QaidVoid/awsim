use std::collections::HashMap;

use awsim_core::tags::{TagOpts, validate_aws_tag_keys, validate_aws_tags};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{
    ApiKey, AppSyncFunction, AppSyncState, DataSource, GraphqlApi, GraphqlType, Resolver,
    SourceApiAssociation, now_iso,
};

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
        "requestMappingTemplate": r.request_mapping_template,
        "responseMappingTemplate": r.response_mapping_template,
    })
}

fn type_to_json(t: &GraphqlType) -> Value {
    json!({
        "name": t.name,
        "definition": t.definition,
        "format": t.format,
        "arn": t.arn,
    })
}

fn function_to_json(f: &AppSyncFunction) -> Value {
    json!({
        "functionId": f.function_id,
        "functionArn": f.function_arn,
        "name": f.name,
        "description": f.description,
        "dataSourceName": f.data_source_name,
        "requestMappingTemplate": f.request_mapping_template,
        "responseMappingTemplate": f.response_mapping_template,
        "functionVersion": f.function_version,
        "createdAt": f.created_at,
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
        types: Vec::new(),
        functions: Vec::new(),
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    Ok(json!({ "graphqlApi": api_to_json(&api) }))
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    if let Some(name) = input["name"].as_str() {
        api.name = name.to_string();
    }
    if let Some(auth) = input["authenticationType"].as_str() {
        api.authentication_type = auth.to_string();
    }

    Ok(json!({ "graphqlApi": api_to_json(&api) }))
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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

    let default_expires = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        (now + 7 * 86400) as i64
    };

    let expires = input["expires"].as_i64().unwrap_or(default_expires);

    let key = ApiKey {
        id: key_id.clone(),
        description,
        expires,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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

pub fn update_api_key(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let id = input["id"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "id is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let key = api
        .api_keys
        .iter_mut()
        .find(|k| k.id == id)
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("API key {} not found", id))
        })?;

    if let Some(desc) = input["description"].as_str() {
        key.description = Some(desc.to_string());
    }
    if let Some(exp) = input["expires"].as_i64() {
        key.expires = exp;
    }

    let result = key_to_json(key);
    Ok(json!({ "apiKey": result }))
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
    let data_source_name = input["dataSourceName"].as_str().unwrap_or("").to_string();

    let resolver = Resolver {
        type_name: type_name.to_string(),
        field_name: field_name.to_string(),
        data_source_name,
        request_mapping_template: input["requestMappingTemplate"]
            .as_str()
            .map(|s| s.to_string()),
        response_mapping_template: input["responseMappingTemplate"]
            .as_str()
            .map(|s| s.to_string()),
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
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
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let resolvers: Vec<Value> = api
        .resolvers
        .iter()
        .filter(|r| r.type_name == type_name)
        .map(resolver_to_json)
        .collect();

    Ok(json!({ "resolvers": resolvers }))
}

pub fn update_resolver(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;
    let field_name = input["fieldName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "fieldName is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let resolver = api
        .resolvers
        .iter_mut()
        .find(|r| r.type_name == type_name && r.field_name == field_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resolver {}.{} not found", type_name, field_name),
            )
        })?;

    if let Some(ds) = input["dataSourceName"].as_str() {
        resolver.data_source_name = ds.to_string();
    }
    if let Some(req) = input["requestMappingTemplate"].as_str() {
        resolver.request_mapping_template = Some(req.to_string());
    }
    if let Some(resp) = input["responseMappingTemplate"].as_str() {
        resolver.response_mapping_template = Some(resp.to_string());
    }

    let result = resolver_to_json(resolver);
    Ok(json!({ "resolver": result }))
}

pub fn delete_resolver(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;
    let field_name = input["fieldName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "fieldName is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let before = api.resolvers.len();
    api.resolvers
        .retain(|r| !(r.type_name == type_name && r.field_name == field_name));
    if api.resolvers.len() == before {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Resolver {}.{} not found", type_name, field_name),
        ));
    }

    Ok(json!({}))
}

// ── GraphQL Types ─────────────────────────────────────────────────────────────

pub fn create_type(
    state: &AppSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let definition = input["definition"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "definition is required"))?;
    let format = input["format"].as_str().unwrap_or("SDL").to_string();

    // Extract name from SDL definition: `type Name {`
    let name = extract_type_name(definition).unwrap_or_else(|| "UnknownType".to_string());

    let arn = format!(
        "arn:aws:appsync:{}:{}:apis/{}/types/{}",
        ctx.region, ctx.account_id, api_id, name
    );

    let gql_type = GraphqlType {
        name: name.clone(),
        definition: Some(definition.to_string()),
        format,
        arn,
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let result = type_to_json(&gql_type);
    api.types.push(gql_type);

    info!(api_id = %api_id, name = %name, "Created GraphQL type");
    Ok(json!({ "type": result }))
}

pub fn get_type(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let gql_type = api
        .types
        .iter()
        .find(|t| t.name == type_name)
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("Type {} not found", type_name))
        })?;

    Ok(json!({ "type": type_to_json(gql_type) }))
}

pub fn list_types(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let types: Vec<Value> = api.types.iter().map(type_to_json).collect();
    Ok(json!({ "types": types }))
}

pub fn delete_type(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let before = api.types.len();
    api.types.retain(|t| t.name != type_name);
    if api.types.len() == before {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Type {} not found", type_name),
        ));
    }

    Ok(json!({}))
}

// ── AppSync Functions ─────────────────────────────────────────────────────────

pub fn create_function(
    state: &AppSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;
    let data_source_name = input["dataSourceName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "dataSourceName is required"))?;

    let function_id = Uuid::new_v4().to_string().replace('-', "")[..26].to_string();
    let function_arn = format!(
        "arn:aws:appsync:{}:{}:apis/{}/functions/{}",
        ctx.region, ctx.account_id, api_id, function_id
    );

    let function = AppSyncFunction {
        function_id: function_id.clone(),
        function_arn,
        name: name.to_string(),
        description: input["description"].as_str().map(|s| s.to_string()),
        data_source_name: data_source_name.to_string(),
        request_mapping_template: input["requestMappingTemplate"]
            .as_str()
            .map(|s| s.to_string()),
        response_mapping_template: input["responseMappingTemplate"]
            .as_str()
            .map(|s| s.to_string()),
        function_version: input["functionVersion"]
            .as_str()
            .unwrap_or("2018-05-29")
            .to_string(),
        created_at: now_iso(),
    };

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let result = function_to_json(&function);
    api.functions.push(function);

    info!(api_id = %api_id, function_id = %function_id, name = %name, "Created AppSync function");
    Ok(json!({ "functionConfiguration": result }))
}

pub fn get_function(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let function_id = input["functionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "functionId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let func = api
        .functions
        .iter()
        .find(|f| f.function_id == function_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Function {} not found", function_id),
            )
        })?;

    Ok(json!({ "functionConfiguration": function_to_json(func) }))
}

pub fn list_functions(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let functions: Vec<Value> = api.functions.iter().map(function_to_json).collect();
    Ok(json!({ "functions": functions }))
}

pub fn delete_function(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let function_id = input["functionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "functionId is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let before = api.functions.len();
    api.functions.retain(|f| f.function_id != function_id);
    if api.functions.len() == before {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Function {} not found", function_id),
        ));
    }

    Ok(json!({}))
}

pub fn update_function(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let function_id = input["functionId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "functionId is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let func = api
        .functions
        .iter_mut()
        .find(|f| f.function_id == function_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Function {} not found", function_id),
            )
        })?;

    if let Some(name) = input["name"].as_str() {
        func.name = name.to_string();
    }
    if let Some(desc) = input["description"].as_str() {
        func.description = Some(desc.to_string());
    }
    if let Some(ds) = input["dataSourceName"].as_str() {
        func.data_source_name = ds.to_string();
    }
    if let Some(req) = input["requestMappingTemplate"].as_str() {
        func.request_mapping_template = Some(req.to_string());
    }
    if let Some(resp) = input["responseMappingTemplate"].as_str() {
        func.response_mapping_template = Some(resp.to_string());
    }

    let result = function_to_json(func);
    Ok(json!({ "functionConfiguration": result }))
}

// ── API Cache ─────────────────────────────────────────────────────────────────

pub fn flush_api_cache(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;

    if !state.apis.contains_key(api_id) {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        ));
    }

    // Stub: no actual cache to flush
    Ok(json!({}))
}

// ── Data Source extras ────────────────────────────────────────────────────────

pub fn get_data_source(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let ds = api
        .data_sources
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Data source {} not found", name),
            )
        })?;

    Ok(json!({ "dataSource": ds_to_json(ds) }))
}

pub fn update_data_source(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let name = input["name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "name is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let ds = api
        .data_sources
        .iter_mut()
        .find(|d| d.name == name)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Data source {} not found", name),
            )
        })?;

    if let Some(t) = input["type"].as_str() {
        ds.data_source_type = t.to_string();
    }
    if let Some(d) = input["description"].as_str() {
        ds.description = Some(d.to_string());
    }

    let result = ds_to_json(ds);
    Ok(json!({ "dataSource": result }))
}

// ── Resolver extras ───────────────────────────────────────────────────────────

pub fn get_resolver(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;
    let field_name = input["fieldName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "fieldName is required"))?;

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let r = api
        .resolvers
        .iter()
        .find(|r| r.type_name == type_name && r.field_name == field_name)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Resolver {}.{} not found", type_name, field_name),
            )
        })?;

    Ok(json!({ "resolver": resolver_to_json(r) }))
}

// ── Type extras ───────────────────────────────────────────────────────────────

pub fn update_type(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let type_name = input["typeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "typeName is required"))?;

    let mut api = state.apis.get_mut(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let t = api
        .types
        .iter_mut()
        .find(|t| t.name == type_name)
        .ok_or_else(|| {
            AwsError::not_found("NotFoundException", format!("Type {} not found", type_name))
        })?;

    if let Some(def) = input["definition"].as_str() {
        t.definition = Some(def.to_string());
    }
    if let Some(format) = input["format"].as_str() {
        t.format = format.to_string();
    }

    let result = type_to_json(t);
    Ok(json!({ "type": result }))
}

// ── Schema introspection ──────────────────────────────────────────────────────

pub fn get_introspection_schema(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let api_id = input["apiId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "apiId is required"))?;
    let format = input["format"].as_str().unwrap_or("SDL").to_string();

    let api = state.apis.get(api_id).ok_or_else(|| {
        AwsError::not_found(
            "NotFoundException",
            format!("GraphQL API {} not found", api_id),
        )
    })?;

    let schema = api
        .schema
        .clone()
        .unwrap_or_else(|| "type Query { hello: String }".to_string());

    let bytes: Vec<u8> = if format == "JSON" {
        serde_json::to_vec(&json!({ "data": { "__schema": { "types": [] } } })).unwrap_or_default()
    } else {
        schema.into_bytes()
    };

    Ok(json!({ "schema": bytes }))
}

// ── Tags ──────────────────────────────────────────────────────────────────────

pub fn tag_resource(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceArn is required"))?;

    validate_aws_tags(&input["tags"], &TagOpts::aws_default())?;

    let mut entry = state.tags.entry(resource_arn.to_string()).or_default();

    if let Some(tags) = input["tags"].as_object() {
        for (k, v) in tags {
            if let Some(v_str) = v.as_str() {
                entry.insert(k.clone(), v_str.to_string());
            }
        }
    }

    Ok(json!({}))
}

pub fn untag_resource(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceArn is required"))?;

    validate_aws_tag_keys(&input["tagKeys"])?;

    if let Some(mut entry) = state.tags.get_mut(resource_arn)
        && let Some(keys) = input["tagKeys"].as_array()
    {
        for k in keys {
            if let Some(s) = k.as_str() {
                entry.remove(s);
            }
        }
    }

    Ok(json!({}))
}

pub fn list_tags_for_resource(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let resource_arn = input["resourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "resourceArn is required"))?;

    let tags = state
        .tags
        .get(resource_arn)
        .map(|e| {
            e.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();

    Ok(json!({ "tags": Value::Object(tags) }))
}

// ── Source API Associations ───────────────────────────────────────────────────

fn association_to_json(a: &SourceApiAssociation) -> Value {
    json!({
        "associationId": a.association_id,
        "associationArn": a.association_arn,
        "sourceApiId": a.source_api_id,
        "mergedApiId": a.merged_api_id,
        "description": a.description,
        "sourceApiAssociationStatus": a.status,
        "lastSuccessfulMergeDate": a.last_successful_merge_date,
    })
}

pub fn associate_merged_graphql_api(
    state: &AppSyncState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let merged_api_id = input["mergedApiIdentifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "mergedApiIdentifier is required")
    })?;
    let source_api_id = input["sourceApiIdentifier"].as_str().ok_or_else(|| {
        AwsError::bad_request("MissingParameter", "sourceApiIdentifier is required")
    })?;

    let association_id = Uuid::new_v4().to_string().replace('-', "")[..20].to_string();
    let association_arn = format!(
        "arn:aws:appsync:{}:{}:apis/{}/sourceApiAssociations/{}",
        ctx.region, ctx.account_id, merged_api_id, association_id
    );

    let assoc = SourceApiAssociation {
        association_id: association_id.clone(),
        association_arn,
        source_api_id: source_api_id.to_string(),
        merged_api_id: merged_api_id.to_string(),
        description: input["description"].as_str().map(|s| s.to_string()),
        status: "SUCCESS".to_string(),
        last_successful_merge_date: now_iso(),
    };

    let result = association_to_json(&assoc);
    state.source_api_associations.insert(association_id, assoc);

    Ok(json!({ "sourceApiAssociation": result }))
}

pub fn get_source_api_association(state: &AppSyncState, input: &Value) -> Result<Value, AwsError> {
    let association_id = input["associationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "associationId is required"))?;

    let assoc = state
        .source_api_associations
        .get(association_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "NotFoundException",
                format!("Source API association {} not found", association_id),
            )
        })?;

    Ok(json!({ "sourceApiAssociation": association_to_json(&assoc) }))
}

pub fn list_source_api_associations(
    state: &AppSyncState,
    input: &Value,
) -> Result<Value, AwsError> {
    let api_id = input["apiId"].as_str().unwrap_or("");

    let summaries: Vec<Value> = state
        .source_api_associations
        .iter()
        .filter(|e| api_id.is_empty() || e.value().merged_api_id == api_id)
        .map(|e| {
            let a = e.value();
            json!({
                "associationId": a.association_id,
                "associationArn": a.association_arn,
                "sourceApiId": a.source_api_id,
                "mergedApiId": a.merged_api_id,
                "description": a.description,
            })
        })
        .collect();

    Ok(json!({ "sourceApiAssociationSummaries": summaries }))
}

pub fn disassociate_merged_graphql_api(
    state: &AppSyncState,
    input: &Value,
) -> Result<Value, AwsError> {
    let association_id = input["associationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "associationId is required"))?;

    if state
        .source_api_associations
        .remove(association_id)
        .is_none()
    {
        return Err(AwsError::not_found(
            "NotFoundException",
            format!("Source API association {} not found", association_id),
        ));
    }

    Ok(json!({ "sourceApiAssociationStatus": "DELETION_SUCCESS" }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the type name from a GraphQL SDL definition like `type Foo { ... }`.
fn extract_type_name(definition: &str) -> Option<String> {
    let trimmed = definition.trim();
    for keyword in &[
        "type ",
        "input ",
        "interface ",
        "enum ",
        "union ",
        "scalar ",
    ] {
        if let Some(rest) = trimmed.strip_prefix(keyword) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_resource_rejects_reserved_aws_prefix() {
        let state = AppSyncState::default();
        let input = json!({
            "resourceArn": "arn:aws:appsync:us-east-1:000000000000:apis/abc",
            "tags": { "aws:internal": "nope" }
        });
        assert!(tag_resource(&state, &input).is_err());
    }

    #[test]
    fn tag_resource_accepts_valid_tags() {
        let state = AppSyncState::default();
        let input = json!({
            "resourceArn": "arn:aws:appsync:us-east-1:000000000000:apis/abc",
            "tags": { "env": "prod" }
        });
        assert!(tag_resource(&state, &input).is_ok());
    }
}
