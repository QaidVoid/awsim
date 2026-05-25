use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{entity_already_exists, no_such_entity},
    ids::now_iso8601,
    state::{IamState, SamlProvider},
};

use super::super::operations::tags::parse_tags;
use super::require_str;

fn provider_to_value(p: &SamlProvider) -> Value {
    let mut v = json!({
        "SAMLProviderArn": p.arn,
        "SAMLMetadataDocument": p.saml_metadata_document,
        "CreateDate": p.create_date,
    });
    if let Some(vu) = &p.valid_until {
        v["ValidUntil"] = Value::String(vu.clone());
    }
    v
}

pub fn create_saml_provider(
    state: &IamState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "Name")?;
    let saml_metadata_document = require_str(input, "SAMLMetadataDocument")?;
    let tags = parse_tags(input)?;

    let arn = format!("arn:aws:iam::{}:saml-provider/{}", ctx.account_id, name);

    if state.saml_providers.contains_key(&arn) {
        return Err(entity_already_exists("SAMLProvider", name));
    }

    let provider = SamlProvider {
        arn: arn.clone(),
        name: name.to_string(),
        saml_metadata_document: saml_metadata_document.to_string(),
        tags,
        create_date: now_iso8601(),
        valid_until: None,
    };

    state.saml_providers.insert(arn.clone(), provider);

    Ok(json!({ "SAMLProviderArn": arn }))
}

pub fn get_saml_provider(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "SAMLProviderArn")?;
    let provider = state
        .saml_providers
        .get(arn)
        .ok_or_else(|| no_such_entity("SAMLProvider", arn))?;
    Ok(provider_to_value(&provider))
}

pub fn list_saml_providers(state: &IamState, _input: &Value) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .saml_providers
        .iter()
        .map(|p| {
            let mut v = json!({
                "Arn": p.arn,
                "CreateDate": p.create_date,
            });
            if let Some(vu) = &p.valid_until {
                v["ValidUntil"] = Value::String(vu.clone());
            }
            v
        })
        .collect();

    Ok(json!({
        "SAMLProviderList": { "member": list }
    }))
}

pub fn delete_saml_provider(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "SAMLProviderArn")?;

    if state.saml_providers.remove(arn).is_none() {
        return Err(no_such_entity("SAMLProvider", arn));
    }

    Ok(json!({}))
}

pub fn update_saml_provider(state: &IamState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "SAMLProviderArn")?;
    let saml_metadata_document = require_str(input, "SAMLMetadataDocument")?;

    let mut provider = state
        .saml_providers
        .get_mut(arn)
        .ok_or_else(|| no_such_entity("SAMLProvider", arn))?;

    provider.saml_metadata_document = saml_metadata_document.to_string();

    Ok(json!({ "SAMLProviderArn": arn }))
}
