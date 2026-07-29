//! The server must answer the first request it ever receives.
//!
//! This is not a hypothetical. A build once set hyper's
//! `header_read_timeout` without registering a timer, which panicked the
//! tokio worker on the very first request. All 2,900 in-tree tests passed,
//! because none of them started the actual binary and sent it a request:
//! they drive an in-process axum router and never touch the connection
//! setup in `serve_with_limits` where the fault lived.
//!
//! So this test deliberately spawns the real binary.

use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Bind port 0 to have the OS hand us a free port, then release it. A
/// fixed port would collide with a developer's own running instance.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Guard that kills the child on drop, so a failing assertion cannot
/// leave a server running.
struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_server(port: u16) -> ServerGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_awsim"))
        .args(["--port", &port.to_string(), "-v", "warn"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awsim binary");
    ServerGuard(child)
}

/// Start a server on a port we can actually get.
///
/// `free_port` releases the port before the server claims it, so another
/// process (including a sibling test) can win the race in between. Retry
/// on a fresh port rather than leaving a flaky test behind.
fn start_server() -> (u16, ServerGuard) {
    let mut last_err = String::new();
    for _ in 0..5 {
        let port = free_port();
        let mut guard = spawn_server(port);
        match try_wait_for_listen(port, &mut guard) {
            Ok(()) => return (port, guard),
            Err(e) => last_err = e,
        }
    }
    panic!("could not start a server on any port: {last_err}");
}

/// Wait for the port to accept a connection, without sending anything.
/// The point of this test is that the *first* request is the one under
/// test, so readiness must not consume it.
fn try_wait_for_listen(port: u16, guard: &mut ServerGuard) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = guard.0.try_wait() {
            let mut err = String::new();
            if let Some(stderr) = guard.0.stderr.as_mut() {
                let _ = stderr.read_to_string(&mut err);
            }
            return Err(format!("server exited before listening: {status}\n{err}"));
        }
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("server did not listen on port {port} within 30s"))
}

#[test]
fn server_answers_its_very_first_request() {
    let (port, _guard) = start_server();

    let body = "Action=GetCallerIdentity&Version=2011-06-15";
    let resp = ureq_post(port, body);

    assert!(
        resp.contains("HTTP/1.1 200"),
        "first request did not get a 200: {resp}"
    );
    assert!(
        resp.contains("GetCallerIdentityResponse"),
        "first response was not a well-formed STS response: {resp}"
    );

    // The worker must still be alive: a panicked tokio worker can take
    // the connection down without taking the process down, so asserting
    // on the process alone would not catch it.
    let second = ureq_post(port, body);
    assert!(
        second.contains("HTTP/1.1 200"),
        "second request failed, suggesting the first killed a worker: {second}"
    );
}

#[test]
fn health_endpoint_answers_first() {
    // Testcontainers modules wait on this endpoint, so it specifically
    // must work on a cold server.
    let (port, _guard) = start_server();

    let resp = ureq_get(port, "/_awsim/health");
    assert!(
        resp.contains("HTTP/1.1 200"),
        "health endpoint failed on a cold server: {resp}"
    );
}

/// Minimal raw HTTP client. Deliberately not an SDK: the failure this
/// guards against was in connection setup, so the test should speak the
/// wire directly rather than through a client that might retry and mask
/// a first-request fault.
fn ureq_post(port: u16, body: &str) -> String {
    use std::io::Write;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect for POST");
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost:{port}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Authorization: AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/sts/aws4_request\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).expect("write request");
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    out
}

fn ureq_get(port: u16, path: &str) -> String {
    use std::io::Write;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect for GET");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("write request");
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    out
}
