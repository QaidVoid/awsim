use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::handler::EcrService;
use crate::operations::auth::validate_authorization_token;

pub fn router(service: Arc<EcrService>) -> axum::Router<()> {
    axum::Router::new()
        .route("/v2/{repo}/blobs/{digest}", axum::routing::get(get_blob))
        .route(
            "/v2/{repo}/referrers/{digest}",
            axum::routing::get(get_referrers),
        )
        .with_state(service)
}

#[derive(Debug, Deserialize)]
struct ReferrersQuery {
    #[serde(rename = "artifactType")]
    artifact_type: Option<String>,
}

/// OCI Distribution Spec referrers API
/// (`GET /v2/<name>/referrers/<digest>`). Walks every image stored
/// under `repo`, looks at the manifest's `subject.digest`, and
/// returns an OCI image index whose `manifests[]` list every manifest
/// that points at the requested digest. The optional `artifactType`
/// query filters the result down to manifests whose
/// `config.mediaType` (or top-level `artifactType`) matches.
async fn get_referrers(
    State(service): State<Arc<EcrService>>,
    Path((repo, digest)): Path<(String, String)>,
    Query(q): Query<ReferrersQuery>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = enforce_authorization(&headers) {
        return err.into_response();
    }
    let store = service.store();
    let mut referrers: Vec<Value> = Vec::new();
    let mut applied_filter = false;
    for ((_, _), state) in store.iter_all() {
        let Some(repository) = state.repositories.get(&repo) else {
            continue;
        };
        for image in repository.images.iter() {
            let Ok(manifest) = serde_json::from_str::<Value>(&image.image_manifest) else {
                continue;
            };
            let subject_digest = manifest
                .get("subject")
                .and_then(|s| s.get("digest"))
                .and_then(Value::as_str);
            if subject_digest != Some(digest.as_str()) {
                continue;
            }
            let artifact_type = manifest
                .get("artifactType")
                .and_then(Value::as_str)
                .or_else(|| {
                    manifest
                        .get("config")
                        .and_then(|c| c.get("mediaType"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("application/vnd.oci.empty.v1+json")
                .to_string();
            if let Some(ref want) = q.artifact_type {
                applied_filter = true;
                if want != &artifact_type {
                    continue;
                }
            }
            let entry = json!({
                "mediaType": image
                    .image_manifest_media_type
                    .clone()
                    .unwrap_or_else(|| "application/vnd.oci.image.manifest.v1+json".to_string()),
                "digest": image.image_digest,
                "size": image.image_size_in_bytes,
                "artifactType": artifact_type,
                "annotations": manifest
                    .get("annotations")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            });
            referrers.push(entry);
        }
        break;
    }

    let body = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": referrers,
    });
    let mut response = Json(body).into_response();
    if applied_filter {
        // OCI spec: signal that the server honored the filter so
        // clients know the response is narrowed.
        response.headers_mut().insert(
            "OCI-Filters-Applied",
            HeaderValue::from_static("artifactType"),
        );
    }
    response
}

/// Validate the `Authorization` header for a registry HTTP request.
///
/// AWS clients send `Authorization: Basic base64(AWS:<token>)` where
/// `<token>` is the body we minted in
/// [`mint_authorization_token`](crate::operations::auth::mint_authorization_token).
/// When the header is absent we leave the request alone — local
/// testing should keep working without round-tripping
/// GetAuthorizationToken first. When the header IS present, a
/// malformed scheme or a forged / expired token is rejected with
/// `401 Unauthorized`.
fn enforce_authorization(headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let Some(auth) = headers.get(header::AUTHORIZATION) else {
        return Ok(());
    };
    let auth = auth.to_str().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "non-ASCII Authorization header".into(),
        )
    })?;
    let rest = auth.strip_prefix("Basic ").ok_or((
        StatusCode::UNAUTHORIZED,
        "Authorization must be Basic".into(),
    ))?;
    let decoded = BASE64.decode(rest.as_bytes()).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Authorization is not valid base64".into(),
        )
    })?;
    let creds = std::str::from_utf8(&decoded).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Authorization is not UTF-8".into(),
        )
    })?;
    // Basic creds are `username:password`; AWS uses the literal
    // "AWS" as the username and the minted token as the password.
    let token = match creds.split_once(':') {
        Some(("AWS", t)) if !t.is_empty() => t,
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Authorization must be AWS:<token>".into(),
            ));
        }
    };
    if let Err(e) = validate_authorization_token(token) {
        return Err((StatusCode::UNAUTHORIZED, e.message));
    }
    Ok(())
}

async fn get_blob(
    State(service): State<Arc<EcrService>>,
    Path((repo, digest)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = enforce_authorization(&headers) {
        return err.into_response();
    }
    let store = service.store();
    let mut maybe_layer = None;
    for ((_, _), state) in store.iter_all() {
        if let Some(repository) = state.repositories.get(&repo)
            && let Some(layer) = repository.layers.get(&digest)
        {
            maybe_layer = Some(layer.value().clone());
            break;
        }
    }

    let layer = match maybe_layer {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, "layer not found").into_response(),
    };

    let bytes = match layer.body.read_all() {
        Ok(b) => b,
        Err(e) => {
            warn!(repo = %repo, digest = %digest, error = %e, "Failed to read ECR layer body");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to read layer").into_response();
        }
    };

    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&layer.media_type) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&digest) {
        headers.insert("Docker-Content-Digest", v);
    }
    response
}

#[cfg(test)]
mod referrers_tests {
    use super::*;
    use crate::state::{ContainerImage, Repository};
    use awsim_core::AccountRegionStore;
    use axum::body::to_bytes;
    use std::collections::HashMap;

    fn empty_service() -> Arc<EcrService> {
        Arc::new(EcrService::new())
    }

    fn seed_repo(svc: &EcrService, images: Vec<ContainerImage>) {
        let store = svc.store();
        let state = store.get("000000000000", "us-east-1");
        state.repositories.insert(
            "demo".into(),
            Repository {
                name: "demo".into(),
                arn: "arn:aws:ecr:us-east-1:000000000000:repository/demo".into(),
                registry_id: "000000000000".into(),
                repository_uri: "000000000000.dkr.ecr.us-east-1.amazonaws.com/demo".into(),
                images,
                layers: dashmap::DashMap::new(),
                created_at: "1970-01-01T00:00:00Z".into(),
                image_tag_mutability: "MUTABLE".into(),
                tags: HashMap::new(),
                lifecycle_policy: None,
                lifecycle_policy_preview: None,
                repository_policy: None,
                scan_on_push: false,
                encryption_type: "AES256".into(),
                kms_key: None,
            },
        );
    }

    fn image(digest: &str, manifest: &str) -> ContainerImage {
        ContainerImage {
            image_digest: digest.into(),
            image_tag: None,
            image_manifest: manifest.into(),
            pushed_at: "1970-01-01T00:00:00Z".into(),
            image_size_in_bytes: 42,
            image_manifest_media_type: Some(
                "application/vnd.oci.image.manifest.v1+json".to_string(),
            ),
        }
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn empty_repository_returns_empty_index() {
        let svc = empty_service();
        seed_repo(&svc, Vec::new());
        let resp = get_referrers(
            State(svc),
            Path(("demo".into(), "sha256:deadbeef".into())),
            Query(ReferrersQuery {
                artifact_type: None,
            }),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["schemaVersion"], 2);
        assert_eq!(body["mediaType"], "application/vnd.oci.image.index.v1+json");
        assert_eq!(body["manifests"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn surfaces_referrers_pointing_at_subject_digest() {
        let svc = empty_service();
        let subject = "sha256:aaaa";
        let referrer = image(
            "sha256:bbbb",
            &format!(
                r#"{{
                    "schemaVersion": 2,
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "config": {{ "mediaType": "application/vnd.example.sbom+json" }},
                    "layers": [],
                    "subject": {{ "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "{subject}", "size": 0 }}
                }}"#
            ),
        );
        let unrelated = image("sha256:cccc", r#"{"schemaVersion":2,"layers":[]}"#);
        seed_repo(&svc, vec![referrer, unrelated]);

        let resp = get_referrers(
            State(svc),
            Path(("demo".into(), subject.into())),
            Query(ReferrersQuery {
                artifact_type: None,
            }),
            HeaderMap::new(),
        )
        .await;
        let body = body_json(resp).await;
        let manifests = body["manifests"].as_array().unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0]["digest"], "sha256:bbbb");
        assert_eq!(
            manifests[0]["artifactType"],
            "application/vnd.example.sbom+json"
        );
    }

    #[tokio::test]
    async fn artifact_type_filter_narrows_results() {
        let svc = empty_service();
        let subject = "sha256:aaaa";
        let sbom = image(
            "sha256:bbbb",
            &format!(
                r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.example.sbom+json"}},"subject":{{"digest":"{subject}"}}}}"#
            ),
        );
        let signature = image(
            "sha256:cccc",
            &format!(
                r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.example.signature+json"}},"subject":{{"digest":"{subject}"}}}}"#
            ),
        );
        seed_repo(&svc, vec![sbom, signature]);

        let resp = get_referrers(
            State(svc),
            Path(("demo".into(), subject.into())),
            Query(ReferrersQuery {
                artifact_type: Some("application/vnd.example.sbom+json".into()),
            }),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(
            resp.headers().get("OCI-Filters-Applied").unwrap(),
            "artifactType"
        );
        let body = body_json(resp).await;
        let manifests = body["manifests"].as_array().unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0]["digest"], "sha256:bbbb");
    }

    #[tokio::test]
    async fn returns_empty_index_for_unknown_repository() {
        let svc = empty_service();
        let resp = get_referrers(
            State(svc),
            Path(("ghost".into(), "sha256:aaaa".into())),
            Query(ReferrersQuery {
                artifact_type: None,
            }),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["manifests"].as_array().unwrap().len(), 0);
    }

    #[allow(dead_code)]
    fn _unused_store_warning_silencer() {
        let _: AccountRegionStore<crate::state::EcrState> = AccountRegionStore::new();
    }
}
