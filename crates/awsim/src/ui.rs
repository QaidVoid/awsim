//! Embedded admin UI.
//!
//! The SvelteKit static build under `ui/build/` is compiled into the binary
//! via `rust-embed`. We mount it under `/_awsim/ui/` so it shares the
//! `_awsim` admin prefix and never collides with AWS service paths.
//!
//! The build is a SPA with a `200.html` fallback: any path that doesn't
//! match a real asset gets the SPA shell, which then hydrates the route
//! client-side. That means dynamic routes (e.g. `/iam/users/[name]`) work
//! without us pre-enumerating every parameter.
//!
//! When the workspace is built without the UI compiled (`ui/build/` empty
//! — common for cargo-only contributors), the embed has no `200.html` and
//! every route returns a short "UI not built" hint with build instructions.

use axum::body::Body;
use axum::extract::{Path, Request};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/build"]
struct UiAssets;

const SPA_FALLBACK: &str = "200.html";

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/_awsim/ui", get(redirect_to_index))
        .route("/_awsim/ui/", get(serve_index))
        .route("/_awsim/ui/{*path}", get(serve_path))
}

/// True when the SvelteKit static build was embedded at compile time.
/// Used to gate the startup banner's UI URL line so we don't advertise
/// a working URL on a binary that will only return the "not built"
/// placeholder.
pub fn is_bundled() -> bool {
    UiAssets::get(SPA_FALLBACK).is_some()
}

/// Layer middleware that redirects browser hits on `/` to the admin UI.
///
/// We can't blindly redirect `/` because AWS SDKs use it for root-level
/// requests (e.g. S3 `ListBuckets` is `GET /` with SigV4 signing). The
/// rule here keys on three browser-only signals so SDK calls fall
/// through to the gateway untouched:
///
/// 1. The path is exactly `/`.
/// 2. There is no AWS SigV4 `Authorization` header.
/// 3. `Accept` advertises `text/html`.
///
/// Any one of those failing means the request is treated as an AWS API
/// call and forwarded to the next layer (the service gateway).
pub async fn root_redirect_middleware(req: Request, next: Next) -> Response {
    if !is_bundled() {
        return next.run(req).await;
    }
    if req.method() == Method::GET
        && req.uri().path() == "/"
        && !has_aws_auth(req.headers())
        && wants_html(req.headers())
    {
        return Redirect::temporary("/_awsim/ui/").into_response();
    }
    next.run(req).await
}

fn has_aws_auth(headers: &HeaderMap) -> bool {
    if let Some(value) = headers.get(header::AUTHORIZATION)
        && let Ok(s) = value.to_str()
    {
        // SigV4 starts with `AWS4-HMAC-SHA256`. Pre-signed URLs put the
        // signature in the query string instead of a header — those
        // never carry an Authorization, so the query-string presence
        // is checked separately below.
        return s.starts_with("AWS4-");
    }
    false
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|a| a.contains("text/html"))
        .unwrap_or(false)
}

async fn redirect_to_index() -> Redirect {
    Redirect::permanent("/_awsim/ui/")
}

async fn serve_index() -> Response {
    serve_asset(SPA_FALLBACK)
}

async fn serve_path(Path(path): Path<String>) -> Response {
    // Try the literal asset first (e.g. `_app/immutable/start.js`,
    // `seed/index.html`). If it's not present, fall back to the SPA
    // shell so client-side routing can take over — same behavior as a
    // typical SvelteKit static-host setup (Netlify, Cloudflare Pages).
    if let Some(file) = UiAssets::get(&path) {
        return file_response(&path, file);
    }
    // SvelteKit emits per-route `*/index.html`. If a request comes in
    // without the trailing `index.html`, look it up explicitly.
    let with_index = if path.ends_with('/') {
        format!("{path}index.html")
    } else {
        format!("{path}/index.html")
    };
    if let Some(file) = UiAssets::get(&with_index) {
        return file_response(&with_index, file);
    }
    serve_asset(SPA_FALLBACK)
}

fn serve_asset(path: &str) -> Response {
    match UiAssets::get(path) {
        Some(file) => file_response(path, file),
        None => not_built_response(),
    }
}

fn file_response(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let body = Body::from(file.data.into_owned());
    let mut response = (StatusCode::OK, body).into_response();
    if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    // Hashed asset paths under `_app/immutable/` are content-addressed,
    // safe to cache aggressively. Everything else (HTML shells, fonts in
    // `static/`) gets a short-lived cache to avoid stale UI after upgrade.
    let cache_value = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_value));
    response
}

fn not_built_response() -> Response {
    let body = "<!DOCTYPE html><html><head><title>AWSim</title></head><body style=\"font-family:sans-serif;max-width:640px;margin:48px auto;padding:0 24px;color:#27272a\"><h1>Admin UI not bundled</h1><p>This <code>awsim</code> binary was built without the UI assets. Build the UI before <code>cargo build</code>:</p><pre style=\"background:#f4f4f5;padding:12px;border-radius:6px\">cd ui &amp;&amp; bun install &amp;&amp; bun run build</pre><p>Or use the published Docker image / release binaries, which always ship with the UI.</p></body></html>";
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}
