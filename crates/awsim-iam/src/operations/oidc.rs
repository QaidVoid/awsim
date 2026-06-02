use crate::{
    error::{entity_already_exists, no_such_entity},
    ids::now_iso8601,
    state::{IamState, OidcProvider},
};
use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use super::super::operations::tags::parse_tags;
use super::require_str;

fn parse_string_list(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.get("member"))
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn provider_to_value(p: &OidcProvider) -> Value {
    let clients: Vec<Value> = p
        .client_id_list
        .iter()
        .map(|c| Value::String(c.clone()))
        .collect();
    let thumbprints: Vec<Value> = p
        .thumbprint_list
        .iter()
        .map(|t| Value::String(t.clone()))
        .collect();
    json!({
        "Url": p.url,
        "ClientIDList": { "member": clients },
        "ThumbprintList": { "member": thumbprints },
        "CreateDate": p.create_date,
    })
}

pub fn create_open_id_connect_provider(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let url = require_str(input, "Url")?;
    let client_id_list = parse_string_list(input, "ClientIDList");
    let thumbprint_list = parse_string_list(input, "ThumbprintList");
    let tags = parse_tags(input)?;

    // Normalize URL: strip trailing slash, strip scheme for the ARN
    let url_clean = url.trim_end_matches('/');
    let url_without_scheme = url_clean
        .strip_prefix("https://")
        .or_else(|| url_clean.strip_prefix("http://"))
        .unwrap_or(url_clean);

    let arn = arn::build_global(ctx, "iam", format!("oidc-provider/{url_without_scheme}"));

    if state.oidc_providers.contains_key(&arn) {
        return Err(entity_already_exists("OIDCProvider", url));
    }

    let provider = OidcProvider {
        arn: arn.clone(),
        url: url_clean.to_string(),
        client_id_list,
        thumbprint_list,
        tags,
        create_date: now_iso8601(),
    };

    state.oidc_providers.insert(arn.clone(), provider);

    Ok(json!({ "OpenIDConnectProviderArn": arn }))
}

pub fn get_open_id_connect_provider(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "OpenIDConnectProviderArn")?;
    let provider = state
        .oidc_providers
        .get(arn)
        .ok_or_else(|| no_such_entity("OIDCProvider", arn))?;
    Ok(provider_to_value(&provider))
}

pub fn list_open_id_connect_providers(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .oidc_providers
        .iter()
        .map(|p| json!({ "Arn": p.arn }))
        .collect();

    Ok(json!({
        "OpenIDConnectProviderList": { "member": list }
    }))
}

pub fn delete_open_id_connect_provider(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "OpenIDConnectProviderArn")?;

    if state.oidc_providers.remove(arn).is_none() {
        return Err(no_such_entity("OIDCProvider", arn));
    }

    Ok(json!({}))
}

pub fn add_client_id_to_open_id_connect_provider(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "OpenIDConnectProviderArn")?;
    let client_id = require_str(input, "ClientID")?;

    let mut provider = state
        .oidc_providers
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("OIDCProvider", arn))?;

    if !provider.client_id_list.contains(&client_id.to_string()) {
        provider.client_id_list.push(client_id.to_string());
    }

    Ok(json!({}))
}

pub fn remove_client_id_from_open_id_connect_provider(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "OpenIDConnectProviderArn")?;
    let client_id = require_str(input, "ClientID")?;

    let mut provider = state
        .oidc_providers
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("OIDCProvider", arn))?;

    let before = provider.client_id_list.len();
    provider.client_id_list.retain(|c| c != client_id);

    if provider.client_id_list.len() == before {
        return Err(no_such_entity("ClientID", client_id));
    }

    Ok(json!({}))
}

pub fn update_open_id_connect_provider_thumbprint(
    state: &IamState,
    input: &Value,
) -> Result<Value, AwsError> {
    let arn = require_str(input, "OpenIDConnectProviderArn")?;
    let thumbprint_list = parse_string_list(input, "ThumbprintList");

    let mut provider = state
        .oidc_providers
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("OIDCProvider", arn))?;

    provider.thumbprint_list = thumbprint_list;

    Ok(json!({}))
}
