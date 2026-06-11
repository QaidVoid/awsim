pub mod auth;
pub mod auth_policy;
pub mod branding;
pub mod client_secrets;
pub mod devices;
pub mod domain;
pub mod groups;
pub mod identity_providers;
pub mod import;
pub mod mfa;
pub mod pools;
pub mod resource_servers;
pub mod risk;
pub mod schema_validation;
pub mod tags;
pub mod terms;
pub mod user_settings;
pub mod users;
pub mod webauthn;

use serde_json::{Value, json};

/// Fill in the standard Cognito trigger-event envelope (version, region,
/// triggerSource, callerContext.awsSdkVersion, empty response block) on top of
/// the caller-supplied request fields, so consumer Lambdas see the AWS shape.
pub(crate) fn cognito_trigger_event(event: &Value, trigger_source: &str, region: &str) -> Value {
    let mut ev = event.clone();
    if let Some(obj) = ev.as_object_mut() {
        obj.entry("version").or_insert_with(|| json!("1"));
        obj.entry("triggerSource")
            .or_insert_with(|| json!(trigger_source));
        obj.entry("region").or_insert_with(|| json!(region));
        obj.entry("response").or_insert_with(|| json!({}));
        if let Some(caller) = obj.get_mut("callerContext").and_then(|c| c.as_object_mut()) {
            caller
                .entry("awsSdkVersion")
                .or_insert_with(|| json!("aws-sdk-unknown"));
        }
    }
    ev
}
