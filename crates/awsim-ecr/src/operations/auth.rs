use awsim_core::{AwsError, RequestContext};
use base64::Engine;
use serde_json::{Value, json};

use crate::operations::repositories::now_epoch_str;

// ---------------------------------------------------------------------------
// GetAuthorizationToken
// ---------------------------------------------------------------------------

pub fn get_authorization_token(
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let credentials = format!("{}:{}", "AWS", uuid::Uuid::new_v4());
    let token = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());

    let proxy_endpoint = format!(
        "https://{}.dkr.ecr.{}.localhost",
        ctx.account_id, ctx.region
    );

    // Expiration: 12 hours from now (in seconds since epoch)
    let expires_at = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + 43200
    };

    let auth_data = json!({
        "authorizationToken": token,
        "expiresAt": expires_at,
        "proxyEndpoint": proxy_endpoint
    });

    Ok(json!({ "authorizationData": [auth_data] }))
}

pub fn _now_epoch_str() -> String {
    now_epoch_str()
}
