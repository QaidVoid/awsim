//! Lambda `Invoke` wire shape.
//!
//! Three defects made invocation unusable from a real SDK:
//!
//! * The Node bootstrap read the event from `process.argv[1]`, which is
//!   the bootstrap's own path. `JSON.parse` threw before the handler ran,
//!   so every invoke returned `FunctionError: Unhandled` carrying node's
//!   crash output.
//! * The event payload is the whole request body, but the handler looked
//!   for a `Payload` member that the wire never carries.
//! * The response body was an envelope rather than the function's own
//!   return value.

use std::sync::Arc;

use awsim_core::{RequestContext, ServiceHandler};
use awsim_lambda::LambdaService;
use base64::Engine;
use serde_json::{Value, json};

fn ctx() -> RequestContext {
    RequestContext::new("lambda", "us-east-1")
}

/// A handler that echoes its event back, so a payload that fails to
/// arrive is visible rather than silently empty.
fn echo_zip() -> String {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        w.start_file("index.js", SimpleFileOptions::default())
            .unwrap();
        w.write_all(
            b"exports.handler = async (event) => {\n\
              if (event && event.boom) { throw new Error('deliberate'); }\n\
              return { echoed: event };\n\
            };\n",
        )
        .unwrap();
        w.finish().unwrap();
    }
    base64::engine::general_purpose::STANDARD.encode(&buf)
}

/// Unique function name per test.
///
/// The extracted-code cache lives at `{tmp}/awsim-lambda/{name}/code`,
/// keyed by function name alone and shared across processes. Two tests
/// using the same name race on it, and a stale-hash miss makes one
/// `remove_dir_all` the directory another is executing from.
fn unique_name(label: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{label}-{nanos}-{}", N.fetch_add(1, Ordering::Relaxed))
}

async fn service_with_echo(name: &str) -> Arc<LambdaService> {
    let svc = Arc::new(LambdaService::new());
    svc.handle(
        "CreateFunction",
        json!({
            "FunctionName": name,
            "Runtime": "nodejs20.x",
            "Handler": "index.handler",
            "Role": "arn:aws:iam::000000000000:role/r",
            "Code": { "ZipFile": echo_zip() },
        }),
        &ctx(),
    )
    .await
    .expect("CreateFunction");
    svc
}

/// Build the input the REST layer produces: the body arrives as
/// `__raw_body`, not as a `Payload` member.
fn wire_invoke(function: &str, body: &Value) -> Value {
    json!({
        "FunctionName": function,
        "__raw_body": base64::engine::general_purpose::STANDARD
            .encode(body.to_string().as_bytes()),
    })
}

fn raw_body(response: &Value) -> Value {
    let encoded = response["__raw_body"].as_str().expect("__raw_body");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("base64");
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes).expect("payload is JSON")
}

/// If the bootstrap cannot read the event, this is the test that says so.
#[tokio::test]
async fn event_payload_reaches_the_handler() {
    let name = unique_name("payload");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle(
            "Invoke",
            wire_invoke(&name, &json!({"hello": "world"})),
            &ctx(),
        )
        .await
        .expect("Invoke");

    assert!(
        out.get("FunctionError").is_none(),
        "handler errored instead of running: {out:?}\nlogs: {:?}",
        out.get("LogResult")
    );
    assert_eq!(
        raw_body(&out),
        json!({ "echoed": { "hello": "world" } }),
        "the event did not round-trip through the handler"
    );
}

/// AWS returns the function's payload as the body, not an envelope. An
/// SDK reads the body directly as the result.
#[tokio::test]
async fn response_body_is_the_payload_not_an_envelope() {
    let name = unique_name("envelope");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle("Invoke", wire_invoke(&name, &json!({"a": 1})), &ctx())
        .await
        .expect("Invoke");

    let body = raw_body(&out);
    assert!(
        body.get("StatusCode").is_none() && body.get("Payload").is_none(),
        "the response body is an envelope rather than the payload: {body:?}"
    );
    assert_eq!(body["echoed"]["a"], json!(1));
}

#[tokio::test]
async fn handler_throw_sets_the_function_error_header() {
    let name = unique_name("throw");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle("Invoke", wire_invoke(&name, &json!({"boom": true})), &ctx())
        .await
        .expect("Invoke");

    assert_eq!(out["FunctionError"], json!("Unhandled"));
    assert_eq!(
        out["__headers"]["X-Amz-Function-Error"],
        json!("Unhandled"),
        "the SDK reads the error from the header, not the body"
    );
    let body = raw_body(&out);
    assert!(
        body.get("errorMessage").is_some(),
        "error payload should carry errorMessage: {body:?}"
    );
}

/// An in-process caller (event source mappings, Secrets Manager
/// rotation) passes `Payload` directly and must keep working.
#[tokio::test]
async fn in_process_payload_member_still_works() {
    let name = unique_name("member");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle(
            "Invoke",
            json!({ "FunctionName": name, "Payload": { "via": "member" } }),
            &ctx(),
        )
        .await
        .expect("Invoke");
    assert_eq!(out["Payload"]["echoed"]["via"], json!("member"));
}

#[tokio::test]
async fn async_invoke_is_202_with_an_empty_body() {
    let name = unique_name("async");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle(
            "Invoke",
            json!({ "FunctionName": name, "InvocationType": "Event" }),
            &ctx(),
        )
        .await
        .expect("Invoke");
    assert_eq!(out["__status_code"], json!(202));
    assert_eq!(
        raw_body(&out),
        Value::Null,
        "Event invoke must return no body"
    );
}

#[tokio::test]
async fn dry_run_is_204() {
    let name = unique_name("dryrun");
    let svc = service_with_echo(&name).await;
    let out = svc
        .handle(
            "Invoke",
            json!({ "FunctionName": name, "InvocationType": "DryRun" }),
            &ctx(),
        )
        .await
        .expect("Invoke");
    assert_eq!(out["__status_code"], json!(204));
}

/// `name:alias` and `name:version` are valid function references
/// everywhere AWS accepts a function name.
#[tokio::test]
async fn invoke_through_an_alias_qualifier() {
    let name = unique_name("alias");
    let svc = service_with_echo(&name).await;
    let published = svc
        .handle("PublishVersion", json!({ "FunctionName": name }), &ctx())
        .await
        .expect("PublishVersion");
    let version = published["Version"].as_str().expect("Version").to_string();

    svc.handle(
        "CreateAlias",
        json!({ "FunctionName": name, "Name": "live", "FunctionVersion": version }),
        &ctx(),
    )
    .await
    .expect("CreateAlias");

    let out = svc
        .handle(
            "Invoke",
            wire_invoke(&format!("{name}:live"), &json!({"q": 1})),
            &ctx(),
        )
        .await
        .expect("invoke through the alias should resolve");
    assert_eq!(raw_body(&out)["echoed"]["q"], json!(1));
}

/// An unknown qualifier must fail rather than quietly running $LATEST,
/// which would execute different code than the caller asked for.
#[tokio::test]
async fn unknown_qualifier_is_rejected() {
    let name = unique_name("badqual");
    let svc = service_with_echo(&name).await;
    let res = svc
        .handle(
            "Invoke",
            wire_invoke(&format!("{name}:nosuchalias"), &json!({})),
            &ctx(),
        )
        .await;
    match res {
        Err(e) => assert_eq!(e.code, "ResourceNotFoundException", "{e:?}"),
        Ok(v) => panic!("unknown qualifier should be rejected, got {v:?}"),
    }
}

#[tokio::test]
async fn memory_and_timeout_bounds_are_enforced() {
    let svc = Arc::new(LambdaService::new());
    let base = |name: &str| {
        json!({
            "FunctionName": name,
            "Runtime": "nodejs20.x",
            "Handler": "index.handler",
            "Role": "arn:aws:iam::000000000000:role/r",
            "Code": { "ZipFile": echo_zip() },
        })
    };

    let mut too_small = base("mem-low");
    too_small["MemorySize"] = json!(64);
    assert!(
        svc.handle("CreateFunction", too_small, &ctx())
            .await
            .is_err(),
        "MemorySize below 128 MB should be rejected"
    );

    let mut too_big = base("mem-high");
    too_big["MemorySize"] = json!(20000);
    assert!(
        svc.handle("CreateFunction", too_big, &ctx()).await.is_err(),
        "MemorySize above 10240 MB should be rejected"
    );

    let mut long = base("to-long");
    long["Timeout"] = json!(1000);
    assert!(
        svc.handle("CreateFunction", long, &ctx()).await.is_err(),
        "Timeout above 900 s should be rejected"
    );

    let mut ok = base("within-bounds");
    ok["MemorySize"] = json!(512);
    ok["Timeout"] = json!(30);
    assert!(
        svc.handle("CreateFunction", ok, &ctx()).await.is_ok(),
        "values inside the bounds must still be accepted"
    );
}
