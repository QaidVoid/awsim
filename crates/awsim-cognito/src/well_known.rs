//! Root-level OpenID discovery endpoints.
//!
//! Tokens minted by the SDK auth flows carry an issuer of the form
//! `https://cognito-idp.{region}.amazonaws.com/{pool_id}`, and OIDC libraries
//! discover the signing keys by fetching `{issuer}/.well-known/jwks.json` (or
//! the `openid-configuration` document that points at it). AWS serves those at
//! the issuer root, so awsim mirrors them at `/{pool_id}/.well-known/*` rather
//! than only under the hosted-UI `/cognito/{pool_id}/...` prefix.
//!
//! The handlers are stateless: the signing key is process-wide and the
//! discovery document is derived from the request's host and path. They are
//! mounted on the gateway router alongside the S3 `/{bucket}/{*key}` catch-all,
//! reusing the same path parameter name so the routers merge without a
//! conflict; the static `.well-known` segment makes them win over the
//! catch-all.

use axum::extract::Path;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::keys;

/// Reconstruct the issuer base (`scheme://host`) from request headers,
/// honouring `x-forwarded-proto` when a proxy terminates TLS.
fn issuer_base(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "http".to_string());
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}

/// `GET /{pool_id}/.well-known/jwks.json`: the public signing key set.
pub async fn jwks(Path(_pool_id): Path<String>) -> Response {
    Json(keys::jwks_document()).into_response()
}

/// Build the minimal OIDC discovery document for `issuer`, matching the shape
/// Cognito serves at the issuer root.
fn openid_document(issuer: &str) -> serde_json::Value {
    json!({
        "issuer": issuer,
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "id_token_signing_alg_values_supported": ["RS256"],
        "response_types_supported": ["code", "token"],
        "subject_types_supported": ["public"],
        "scopes_supported": ["openid", "email", "phone", "profile"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "claims_supported": [
            "sub", "iss", "aud", "exp", "iat", "auth_time", "token_use",
            "cognito:username", "cognito:groups", "email", "email_verified"
        ]
    })
}

/// `GET /{pool_id}/.well-known/openid-configuration`: minimal OIDC discovery
/// document pointing at the pool's JWKS endpoint, matching the shape Cognito
/// serves at the issuer root.
pub async fn openid_configuration(Path(pool_id): Path<String>, headers: HeaderMap) -> Response {
    let issuer = format!("{}/{}", issuer_base(&headers), pool_id);
    Json(openid_document(&issuer)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openid_document_points_at_pool_jwks() {
        let doc = openid_document("https://cognito-idp.us-east-1.amazonaws.com/us-east-1_abc");
        assert_eq!(
            doc["issuer"],
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_abc"
        );
        assert_eq!(
            doc["jwks_uri"],
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_abc/.well-known/jwks.json"
        );
        assert_eq!(doc["id_token_signing_alg_values_supported"][0], "RS256");
    }

    #[test]
    fn issuer_base_honours_forwarded_proto() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "example.test".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(issuer_base(&h), "https://example.test");
    }

    // The root discovery routes must coexist with the S3 `/{bucket}/{*key}`
    // catch-all in one matchit router (same parameter name, static segment
    // wins). Router construction inserts the routes eagerly, so a conflict
    // would panic here.
    #[test]
    fn root_routes_coexist_with_s3_catchall() {
        let _: axum::Router = axum::Router::new()
            .route("/{bucket}/.well-known/jwks.json", axum::routing::get(jwks))
            .route(
                "/{bucket}/.well-known/openid-configuration",
                axum::routing::get(openid_configuration),
            )
            .route("/{bucket}/{*key}", axum::routing::any(|| async { "ok" }));
    }
}
