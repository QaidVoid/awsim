//! Regression coverage for archive extraction containment.
//!
//! A deployment package is fully attacker-controlled, and extraction writes
//! to the filesystem. These tests pin the containment guarantees: no member
//! may write outside the cache directory, symlink members are refused, and
//! a function name cannot redirect the cache path.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{RequestContext, ServiceHandler};
use awsim_lambda::LambdaService;
use base64::Engine;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-lambda-extract-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("lambda", "us-east-1")
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Build a zip whose members are given verbatim, so traversal names survive
/// into the archive rather than being normalised by a helper.
fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, payload) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(payload).unwrap();
        }
        writer.finish().unwrap();
    }
    buf
}

/// A zip carrying a symlink member pointing at `target`.
fn zip_with_symlink(link_name: &str, target: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        writer
            .add_symlink(link_name, target, SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
    }
    buf
}

async fn create_and_invoke(svc: &LambdaService, name: &str, zip_bytes: &[u8]) -> Option<Value> {
    svc.handle(
        "CreateFunction",
        json!({
            "FunctionName": name,
            "Role": "arn:aws:iam::000000000000:role/lambda",
            "Runtime": "nodejs18.x",
            "Handler": "index.handler",
            "Code": { "ZipFile": b64(zip_bytes) },
        }),
        &ctx(),
    )
    .await
    .ok()?;

    svc.handle("Invoke", json!({ "FunctionName": name }), &ctx())
        .await
        .ok()
}

/// The traversal target used across tests. Chosen under the temp dir so a
/// regression cannot damage anything outside it.
fn traversal_target(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("awsim-extract-escape-{label}.txt"))
}

/// Relative `..` traversal must not write outside the destination.
#[tokio::test]
async fn relative_traversal_entry_does_not_escape() {
    let dir = tmp_dir("relative");
    let target = traversal_target("relative");
    let _ = std::fs::remove_file(&target);

    let rel = format!(
        "../../../../../../../../../../../..{}",
        target.to_string_lossy()
    );
    let zip_bytes = zip_with_entries(&[
        ("index.js", b"exports.handler=async()=>({});" as &[u8]),
        (rel.as_str(), b"escaped" as &[u8]),
    ]);

    let svc = LambdaService::with_data_dir(&dir);
    let _ = create_and_invoke(&svc, "reltraversal", &zip_bytes).await;

    assert!(
        !target.exists(),
        "relative traversal wrote outside the cache dir at {}",
        target.display()
    );
}

/// An absolute member name must not be honoured. `Path::join` with an
/// absolute path discards the base entirely, which is what made this
/// exploitable.
#[tokio::test]
async fn absolute_path_entry_does_not_escape() {
    let dir = tmp_dir("absolute");
    let target = traversal_target("absolute");
    let _ = std::fs::remove_file(&target);

    let abs = target.to_string_lossy().to_string();
    let zip_bytes = zip_with_entries(&[
        ("index.js", b"exports.handler=async()=>({});" as &[u8]),
        (abs.as_str(), b"escaped" as &[u8]),
    ]);

    let svc = LambdaService::with_data_dir(&dir);
    let _ = create_and_invoke(&svc, "abstraversal", &zip_bytes).await;

    assert!(
        !target.exists(),
        "absolute entry wrote outside the cache dir at {}",
        target.display()
    );
}

/// Traversal hidden behind legitimate-looking leading segments.
#[tokio::test]
async fn nested_traversal_entry_does_not_escape() {
    let dir = tmp_dir("nested");
    let target = traversal_target("nested");
    let _ = std::fs::remove_file(&target);

    let nested = format!(
        "a/b/c/../../../../../../../../../../../../..{}",
        target.to_string_lossy()
    );
    let zip_bytes = zip_with_entries(&[
        ("index.js", b"exports.handler=async()=>({});" as &[u8]),
        (nested.as_str(), b"escaped" as &[u8]),
    ]);

    let svc = LambdaService::with_data_dir(&dir);
    let _ = create_and_invoke(&svc, "nestedtraversal", &zip_bytes).await;

    assert!(
        !target.exists(),
        "nested traversal wrote outside the cache dir at {}",
        target.display()
    );
}

/// A symlink member escapes containment even when every individual name
/// passes a component check, so symlinks are refused outright.
#[tokio::test]
async fn symlink_entry_is_refused() {
    let dir = tmp_dir("symlink");
    let escape_dir = tmp_dir("symlink-target");
    let zip_bytes = zip_with_symlink("link", &escape_dir.to_string_lossy());

    let svc = LambdaService::with_data_dir(&dir);
    let _ = create_and_invoke(&svc, "symlinkfn", &zip_bytes).await;

    let link = std::env::temp_dir()
        .join("awsim-lambda")
        .join("symlinkfn")
        .join("code")
        .join("link");
    assert!(
        !link.is_symlink(),
        "symlink member was materialised at {}",
        link.display()
    );
}

/// Containment must not break ordinary packages.
#[tokio::test]
async fn legitimate_nested_entries_still_extract() {
    let dir = tmp_dir("legit");
    let zip_bytes = zip_with_entries(&[
        ("index.js", b"exports.handler=async()=>({ok:1});" as &[u8]),
        ("lib/helper.js", b"module.exports={};" as &[u8]),
        ("lib/deep/nested.json", b"{}" as &[u8]),
    ]);

    let svc = LambdaService::with_data_dir(&dir);
    let created = svc
        .handle(
            "CreateFunction",
            json!({
                "FunctionName": "legitfn",
                "Role": "arn:aws:iam::000000000000:role/lambda",
                "Runtime": "nodejs18.x",
                "Handler": "index.handler",
                "Code": { "ZipFile": b64(&zip_bytes) },
            }),
            &ctx(),
        )
        .await;
    assert!(
        created.is_ok(),
        "legitimate package was rejected: {created:?}"
    );

    let _ = svc
        .handle("Invoke", json!({ "FunctionName": "legitfn" }), &ctx())
        .await;

    let code_dir = std::env::temp_dir()
        .join("awsim-lambda")
        .join("legitfn")
        .join("code");
    assert!(
        code_dir.join("index.js").exists(),
        "top-level entry missing after extraction"
    );
    assert!(
        code_dir.join("lib").join("helper.js").exists(),
        "nested entry missing after extraction"
    );
    assert!(
        code_dir
            .join("lib")
            .join("deep")
            .join("nested.json")
            .exists(),
        "deeply nested entry missing after extraction"
    );
}

/// A function name is caller-supplied and reaches a filesystem path, so
/// names outside the AWS character set are rejected at the API boundary.
#[tokio::test]
async fn traversing_function_name_is_rejected() {
    let dir = tmp_dir("badname");
    let zip_bytes = zip_with_entries(&[("index.js", b"exports.handler=async()=>({});" as &[u8])]);
    let svc = LambdaService::with_data_dir(&dir);

    for bad in [
        "../escape",
        "../../etc/cron.d/x",
        "a/b",
        "/absolute",
        "has space",
        "",
    ] {
        let res = svc
            .handle(
                "CreateFunction",
                json!({
                    "FunctionName": bad,
                    "Role": "arn:aws:iam::000000000000:role/lambda",
                    "Runtime": "nodejs18.x",
                    "Handler": "index.handler",
                    "Code": { "ZipFile": b64(&zip_bytes) },
                }),
                &ctx(),
            )
            .await;
        assert!(
            res.is_err(),
            "function name `{bad}` should have been rejected"
        );
    }
}

/// The allowlist must still accept every name AWS accepts.
#[tokio::test]
async fn valid_function_names_are_accepted() {
    let dir = tmp_dir("goodname");
    let zip_bytes = zip_with_entries(&[("index.js", b"exports.handler=async()=>({});" as &[u8])]);
    let svc = LambdaService::with_data_dir(&dir);

    for good in ["simple", "with-dash", "with_underscore", "Mixed123"] {
        let res = svc
            .handle(
                "CreateFunction",
                json!({
                    "FunctionName": good,
                    "Role": "arn:aws:iam::000000000000:role/lambda",
                    "Runtime": "nodejs18.x",
                    "Handler": "index.handler",
                    "Code": { "ZipFile": b64(&zip_bytes) },
                }),
                &ctx(),
            )
            .await;
        assert!(
            res.is_ok(),
            "function name `{good}` should be accepted: {res:?}"
        );
    }
}

/// `join_safe` is the shared containment authority. Pin its behaviour here
/// too, so a change to it surfaces against the extraction contract.
#[test]
fn join_safe_contains_every_input() {
    let base = Path::new("/tmp/awsim-base");

    // Traversal components and empty keys are refused outright.
    for bad in ["../x", "a/../../x", "..", ""] {
        assert!(
            awsim_core::join_safe(base, bad).is_err(),
            "join_safe accepted escaping input `{bad}`"
        );
    }

    // An absolute-looking key is treated as relative to base rather than
    // refused, because S3 keys routinely carry a leading slash. The
    // guarantee is containment, not rejection.
    for contained in ["a", "a/b", "a/b/c.txt", "/etc/passwd", "//leading"] {
        let joined = awsim_core::join_safe(base, contained)
            .unwrap_or_else(|e| panic!("join_safe rejected `{contained}`: {e}"));
        assert!(
            joined.starts_with(base),
            "join_safe produced a path outside base for `{contained}`: {}",
            joined.display()
        );
    }
}
