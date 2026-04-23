use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{CognitoState, TermsEntry};

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_links(input: &Value) -> HashMap<String, String> {
    let mut links = HashMap::new();
    if let Some(obj) = input.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                links.insert(k.clone(), s.to_string());
            }
        }
    }
    links
}

fn terms_to_value(t: &TermsEntry) -> Value {
    let links: HashMap<String, Value> = t
        .links
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    json!({
        "TermsId": t.terms_id,
        "UserPoolId": t.user_pool_id,
        "ClientId": t.client_id,
        "TermsName": t.terms_name,
        "TermsSource": t.terms_source,
        "Enforcement": t.enforcement,
        "Links": links,
        "CreationDate": t.creation_date,
        "LastModifiedDate": t.last_modified_date
    })
}

pub fn create_terms(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let terms_name = input["TermsName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TermsName is required"))?;
    let terms_source = input["TermsSource"].as_str().unwrap_or("LINK").to_string();
    let enforcement = input["Enforcement"].as_str().unwrap_or("NONE").to_string();
    let client_id = input["ClientId"].as_str().map(String::from);
    let links = parse_links(&input["Links"]);
    let now = now_epoch();

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let entry = TermsEntry {
        terms_id: Uuid::new_v4().to_string(),
        user_pool_id: pool_id.to_string(),
        client_id,
        terms_name: terms_name.to_string(),
        terms_source,
        enforcement,
        links,
        creation_date: now,
        last_modified_date: now,
    };
    let val = terms_to_value(&entry);
    pool.terms.push(entry);

    info!(pool_id = %pool_id, "Cognito: created terms");
    Ok(json!({ "Terms": val }))
}

pub fn update_terms(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let terms_id = input["TermsId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TermsId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let entry = pool
        .terms
        .iter_mut()
        .find(|t| t.terms_id == terms_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Terms not found: {terms_id}")))?;

    if let Some(name) = input["TermsName"].as_str() {
        entry.terms_name = name.to_string();
    }
    if let Some(src) = input["TermsSource"].as_str() {
        entry.terms_source = src.to_string();
    }
    if let Some(enf) = input["Enforcement"].as_str() {
        entry.enforcement = enf.to_string();
    }
    if !input["Links"].is_null() {
        entry.links = parse_links(&input["Links"]);
    }
    entry.last_modified_date = now_epoch();

    let val = terms_to_value(entry);
    info!(pool_id = %pool_id, terms_id = %terms_id, "Cognito: updated terms");
    Ok(json!({ "Terms": val }))
}

pub fn delete_terms(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let terms_id = input["TermsId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TermsId is required"))?;

    let mut pool = state.user_pools.get_mut(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let len_before = pool.terms.len();
    pool.terms.retain(|t| t.terms_id != terms_id);
    if pool.terms.len() == len_before {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("Terms not found: {terms_id}"),
        ));
    }

    info!(pool_id = %pool_id, terms_id = %terms_id, "Cognito: deleted terms");
    Ok(json!({}))
}

pub fn describe_terms(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let terms_id = input["TermsId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TermsId is required"))?;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let entry = pool
        .terms
        .iter()
        .find(|t| t.terms_id == terms_id)
        .ok_or_else(|| AwsError::not_found("ResourceNotFoundException", format!("Terms not found: {terms_id}")))?;

    Ok(json!({ "Terms": terms_to_value(entry) }))
}

pub fn list_terms(
    state: &CognitoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let pool_id = input["UserPoolId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "UserPoolId is required"))?;
    let max_results = input["MaxResults"].as_u64().unwrap_or(60) as usize;

    let pool = state.user_pools.get(pool_id).ok_or_else(|| {
        AwsError::not_found("ResourceNotFoundException", format!("User pool not found: {pool_id}"))
    })?;

    let entries: Vec<Value> = pool.terms.iter().take(max_results).map(terms_to_value).collect();
    Ok(json!({ "Terms": entries, "NextToken": Value::Null }))
}
