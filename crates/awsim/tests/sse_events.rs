use std::sync::Arc;
use std::time::Duration;

use awsim_core::AppState;
use serde_json::Value;

async fn start_minimal_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("http://{addr}");

    let mut state = AppState::new("us-east-1".into(), "000000000000".into());
    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);

    let events_state = state.clone();
    let app = axum::Router::new()
        .route(
            "/_awsim/events",
            axum::routing::get(
                |axum::extract::State(state): axum::extract::State<AppState>| async move {
                    use axum::response::sse::{Event, KeepAlive, Sse};
                    use std::convert::Infallible;
                    use tokio_stream::StreamExt;
                    use tokio_stream::wrappers::BroadcastStream;
                    let receiver = state.events.subscribe();
                    let stream = BroadcastStream::new(receiver)
                        .filter_map(|res| {
                            res.ok()
                                .and_then(|evt| Event::default().json_data(&evt).ok())
                        })
                        .map(Ok::<_, Infallible>);
                    Sse::new(stream).keep_alive(KeepAlive::default())
                },
            ),
        )
        .fallback(awsim_core::gateway::handle_request)
        .with_state(events_state)
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive());

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    endpoint
}

#[tokio::test]
async fn sse_endpoint_streams_request_events() {
    let endpoint = start_minimal_server().await;

    let sse_endpoint = endpoint.clone();
    let reader = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{sse_endpoint}/_awsim/events"))
            .send()
            .await
            .expect("connect SSE");
        assert_eq!(resp.status(), 200);
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            ctype.starts_with("text/event-stream"),
            "unexpected content-type: {ctype}"
        );

        let mut events: Vec<Value> = Vec::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        use tokio_stream::StreamExt;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while events.len() < 2 && tokio::time::Instant::now() < deadline {
            let next = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                stream.next(),
            )
            .await;
            let Ok(Some(chunk)) = next else { break };
            let bytes = chunk.expect("chunk");
            buf.push_str(std::str::from_utf8(&bytes).unwrap_or(""));
            while let Some(idx) = buf.find("\n\n") {
                let frame: String = buf.drain(..idx + 2).collect();
                for line in frame.lines() {
                    if let Some(rest) = line.strip_prefix("data:") {
                        let json_str = rest.trim();
                        if let Ok(v) = serde_json::from_str::<Value>(json_str) {
                            events.push(v);
                        }
                    }
                }
            }
        }
        events
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let trigger = endpoint.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        for _ in 0..5 {
            let _ = client
                .post(format!("{trigger}/"))
                .header(
                    "x-amz-target",
                    "AWSSecurityTokenServiceV20110615.GetCallerIdentity",
                )
                .header("content-type", "application/x-amz-json-1.1")
                .body("{}")
                .send()
                .await;
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    });

    let events = tokio::time::timeout(Duration::from_secs(6), reader)
        .await
        .expect("reader timeout")
        .expect("reader join");

    assert!(
        events.len() >= 2,
        "expected at least 2 SSE events, got {}: {events:?}",
        events.len()
    );
    for ev in &events {
        assert!(
            ev.get("id").and_then(|v| v.as_str()).is_some(),
            "missing id: {ev}"
        );
        assert!(
            ev.get("ts").and_then(|v| v.as_f64()).is_some(),
            "missing ts: {ev}"
        );
        assert!(
            ev.get("status_code").and_then(|v| v.as_u64()).is_some(),
            "missing status_code: {ev}"
        );
        assert!(
            ev.get("method").and_then(|v| v.as_str()).is_some(),
            "missing method: {ev}"
        );
    }
}
