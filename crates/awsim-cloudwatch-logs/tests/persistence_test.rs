use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_cloudwatch_logs::CloudWatchLogsService;
use awsim_core::{RequestContext, ServiceHandler};
use serde_json::json;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-logs-persist-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RequestContext {
    RequestContext::new("logs", "us-east-1")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::test]
async fn put_then_restart_then_get_round_trips_events() {
    let dir = tmp_dir("round-trip");
    let group = "persist-group";
    let stream = "stream-1";

    // PutLogEvents rejects timestamps outside the 14-day / 2-hour
    // ingestion window, so anchor on the current clock and stagger by
    // small offsets (in chronological order) to exercise persistence.
    let base = now_ms();
    let snapshot = {
        let svc = CloudWatchLogsService::with_data_dir(&dir);
        svc.handle("CreateLogGroup", json!({ "logGroupName": group }), &ctx())
            .await
            .unwrap();
        svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": group, "logStreamName": stream }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": group,
                "logStreamName": stream,
                "logEvents": [
                    { "timestamp": base - 5000, "message": "first" },
                    { "timestamp": base - 4000, "message": "second" },
                    { "timestamp": base - 3000, "message": "third" },
                    { "timestamp": base - 2000, "message": "fourth" },
                    { "timestamp": base - 1000, "message": "fifth" },
                ],
            }),
            &ctx(),
        )
        .await
        .unwrap();
        svc.snapshot().expect("snapshot bytes")
    };

    // Events live in cloudwatch-logs.db now (not under a body store).
    let db_path = dir.join("cloudwatch-logs.db");
    assert!(db_path.exists(), "sqlite file should exist at {db_path:?}");

    let svc2 = CloudWatchLogsService::with_data_dir(&dir);
    svc2.restore(&snapshot).expect("restore");

    let got = svc2
        .handle(
            "GetLogEvents",
            json!({
                "logGroupName": group,
                "logStreamName": stream,
                "startFromHead": true,
            }),
            &ctx(),
        )
        .await
        .unwrap();

    let events = got["events"].as_array().expect("events array");
    assert_eq!(events.len(), 5);
    let want: Vec<(u64, &str)> = vec![
        (base - 5000, "first"),
        (base - 4000, "second"),
        (base - 3000, "third"),
        (base - 2000, "fourth"),
        (base - 1000, "fifth"),
    ];
    for (i, (ts, msg)) in want.into_iter().enumerate() {
        assert_eq!(events[i]["timestamp"].as_u64(), Some(ts), "ts at idx {i}");
        assert_eq!(events[i]["message"].as_str(), Some(msg), "msg at idx {i}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn delete_log_stream_removes_persisted_events() {
    let dir = tmp_dir("delete-stream");
    let group = "persist-del-stream";
    let stream = "to-delete";

    let svc = CloudWatchLogsService::with_data_dir(&dir);
    svc.handle("CreateLogGroup", json!({ "logGroupName": group }), &ctx())
        .await
        .unwrap();
    svc.handle(
        "CreateLogStream",
        json!({ "logGroupName": group, "logStreamName": stream }),
        &ctx(),
    )
    .await
    .unwrap();
    svc.handle(
        "PutLogEvents",
        json!({
            "logGroupName": group,
            "logStreamName": stream,
            "logEvents": [{ "timestamp": now_ms(), "message": "x" }],
        }),
        &ctx(),
    )
    .await
    .unwrap();

    // Re-create the stream after delete and confirm GetLogEvents
    // returns empty — the row would still be there if delete didn't
    // hit SQLite.
    svc.handle(
        "DeleteLogStream",
        json!({ "logGroupName": group, "logStreamName": stream }),
        &ctx(),
    )
    .await
    .unwrap();
    svc.handle(
        "CreateLogStream",
        json!({ "logGroupName": group, "logStreamName": stream }),
        &ctx(),
    )
    .await
    .unwrap();
    let got = svc
        .handle(
            "GetLogEvents",
            json!({
                "logGroupName": group,
                "logStreamName": stream,
                "startFromHead": true,
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(got["events"].as_array().unwrap().len(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn delete_log_group_removes_persisted_events() {
    let dir = tmp_dir("delete-group");
    let group = "persist-del-group";
    let stream = "s";

    let svc = CloudWatchLogsService::with_data_dir(&dir);
    svc.handle("CreateLogGroup", json!({ "logGroupName": group }), &ctx())
        .await
        .unwrap();
    svc.handle(
        "CreateLogStream",
        json!({ "logGroupName": group, "logStreamName": stream }),
        &ctx(),
    )
    .await
    .unwrap();
    svc.handle(
        "PutLogEvents",
        json!({
            "logGroupName": group,
            "logStreamName": stream,
            "logEvents": [{ "timestamp": now_ms(), "message": "y" }],
        }),
        &ctx(),
    )
    .await
    .unwrap();

    svc.handle("DeleteLogGroup", json!({ "logGroupName": group }), &ctx())
        .await
        .unwrap();

    // Re-create + read should yield no surviving events.
    svc.handle("CreateLogGroup", json!({ "logGroupName": group }), &ctx())
        .await
        .unwrap();
    svc.handle(
        "CreateLogStream",
        json!({ "logGroupName": group, "logStreamName": stream }),
        &ctx(),
    )
    .await
    .unwrap();
    let got = svc
        .handle(
            "GetLogEvents",
            json!({
                "logGroupName": group,
                "logStreamName": stream,
                "startFromHead": true,
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert_eq!(got["events"].as_array().unwrap().len(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}
