mod handler;
mod operations;
pub mod sqlite_store;
mod state;

pub use handler::CloudWatchLogsService;
pub use sqlite_store::{LogEventRow, SqliteStore};

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::CloudWatchLogsService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("logs", "us-east-1")
    }

    fn now_ts() -> u64 {
        super::state::now_millis()
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Log Groups
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_log_group_basic() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/my/app/logs" }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn test_create_log_group_with_tags() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/tagged/group", "tags": { "env": "test" } }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle(
            "ListTagsLogGroup",
            json!({ "logGroupName": "/tagged/group" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags["tags"]["env"].as_str().unwrap(), "test");
    }

    #[test]
    fn test_create_log_group_duplicate() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/dup/group" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/dup/group" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceAlreadyExistsException");
    }

    #[test]
    fn test_delete_log_group() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/delete/me" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DeleteLogGroup",
            json!({ "logGroupName": "/delete/me" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle("DescribeLogGroups", json!({}), &ctx)).unwrap();
        assert_eq!(result["logGroups"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_log_group() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("DeleteLogGroup", json!({ "logGroupName": "/ghost" }), &ctx))
            .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_describe_log_groups_empty() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        let result = block_on(svc.handle("DescribeLogGroups", json!({}), &ctx)).unwrap();
        assert_eq!(result["logGroups"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_describe_log_groups_prefix_filter() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/app/foo" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/app/bar" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/other/baz" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "DescribeLogGroups",
            json!({ "logGroupNamePrefix": "/app/" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["logGroups"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_put_retention_policy() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/ret/group" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "PutRetentionPolicy",
            json!({ "logGroupName": "/ret/group", "retentionInDays": 7 }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle("DescribeLogGroups", json!({}), &ctx)).unwrap();
        let group = &result["logGroups"].as_array().unwrap()[0];
        assert_eq!(group["retentionInDays"].as_u64().unwrap(), 7);
    }

    #[test]
    fn test_put_retention_policy_invalid_days() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/ret2/group" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "PutRetentionPolicy",
            json!({ "logGroupName": "/ret2/group", "retentionInDays": 99 }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_delete_retention_policy() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/delret/group" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "PutRetentionPolicy",
            json!({ "logGroupName": "/delret/group", "retentionInDays": 30 }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DeleteRetentionPolicy",
            json!({ "logGroupName": "/delret/group" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle("DescribeLogGroups", json!({}), &ctx)).unwrap();
        let group = &result["logGroups"].as_array().unwrap()[0];
        assert!(group.get("retentionInDays").is_none() || group["retentionInDays"].is_null());
    }

    #[test]
    fn test_tag_untag_log_group() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/tag/group" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "TagLogGroup",
            json!({ "logGroupName": "/tag/group", "tags": { "key1": "val1", "key2": "val2" } }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle(
            "ListTagsLogGroup",
            json!({ "logGroupName": "/tag/group" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags["tags"].as_object().unwrap().len(), 2);

        block_on(svc.handle(
            "UntagLogGroup",
            json!({ "logGroupName": "/tag/group", "tags": ["key1"] }),
            &ctx,
        ))
        .unwrap();

        let tags2 = block_on(svc.handle(
            "ListTagsLogGroup",
            json!({ "logGroupName": "/tag/group" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(tags2["tags"].as_object().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Log Streams
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_log_stream() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/stream/group" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": "/stream/group", "logStreamName": "stream-1" }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn test_create_log_stream_duplicate() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/dup/stream/group" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": "/dup/stream/group", "logStreamName": "dup-stream" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": "/dup/stream/group", "logStreamName": "dup-stream" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceAlreadyExistsException");
    }

    #[test]
    fn test_delete_log_stream() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/del/stream/group" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": "/del/stream/group", "logStreamName": "del-stream" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "DeleteLogStream",
            json!({ "logGroupName": "/del/stream/group", "logStreamName": "del-stream" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle(
            "DescribeLogStreams",
            json!({ "logGroupName": "/del/stream/group" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["logStreams"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_describe_log_streams_prefix() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/desc/streams" }),
            &ctx,
        ))
        .unwrap();

        for name in &["app-stream-1", "app-stream-2", "other-stream"] {
            block_on(svc.handle(
                "CreateLogStream",
                json!({ "logGroupName": "/desc/streams", "logStreamName": name }),
                &ctx,
            ))
            .unwrap();
        }

        let result = block_on(svc.handle(
            "DescribeLogStreams",
            json!({ "logGroupName": "/desc/streams", "logStreamNamePrefix": "app-" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["logStreams"].as_array().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // Log Events
    // -----------------------------------------------------------------------

    fn setup_group_and_stream(svc: &CloudWatchLogsService, group: &str, stream: &str) {
        let ctx = ctx();
        block_on(svc.handle("CreateLogGroup", json!({ "logGroupName": group }), &ctx)).unwrap();
        block_on(svc.handle(
            "CreateLogStream",
            json!({ "logGroupName": group, "logStreamName": stream }),
            &ctx,
        ))
        .unwrap();
    }

    #[test]
    fn test_put_and_get_log_events() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/events/group", "events-stream");

        let now = now_ts();
        let result = block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/events/group",
                "logStreamName": "events-stream",
                "logEvents": [
                    { "timestamp": now - 2000, "message": "first event" },
                    { "timestamp": now - 1000, "message": "second event" },
                ],
            }),
            &ctx,
        ))
        .unwrap();
        assert!(result["nextSequenceToken"].as_str().is_some());

        let got = block_on(svc.handle(
            "GetLogEvents",
            json!({
                "logGroupName": "/events/group",
                "logStreamName": "events-stream",
                "startFromHead": true,
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(got["events"].as_array().unwrap().len(), 2);
        assert_eq!(got["events"][0]["message"].as_str().unwrap(), "first event");
    }

    #[test]
    fn test_get_log_events_time_filter() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/time/group", "time-stream");

        let now = now_ts();
        block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/time/group",
                "logStreamName": "time-stream",
                "logEvents": [
                    { "timestamp": now - 9000, "message": "before" },
                    { "timestamp": now - 5000, "message": "during" },
                    { "timestamp": now - 1000, "message": "after" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle(
            "GetLogEvents",
            json!({
                "logGroupName": "/time/group",
                "logStreamName": "time-stream",
                "startTime": now - 8000,
                "endTime": now - 2000,
                "startFromHead": true,
            }),
            &ctx,
        ))
        .unwrap();
        let events = got["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["message"].as_str().unwrap(), "during");
    }

    #[test]
    fn test_filter_log_events_pattern() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/filter/group", "filter-stream");

        let now = now_ts();
        block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/filter/group",
                "logStreamName": "filter-stream",
                "logEvents": [
                    { "timestamp": now - 3000, "message": "ERROR something failed" },
                    { "timestamp": now - 2000, "message": "INFO all good" },
                    { "timestamp": now - 1000, "message": "ERROR another failure" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle(
            "FilterLogEvents",
            json!({
                "logGroupName": "/filter/group",
                "filterPattern": "ERROR",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(got["events"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_filter_log_events_empty_pattern() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/filter2/group", "filter2-stream");

        let now = now_ts();
        block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/filter2/group",
                "logStreamName": "filter2-stream",
                "logEvents": [
                    { "timestamp": now - 2000, "message": "msg1" },
                    { "timestamp": now - 1000, "message": "msg2" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let got = block_on(svc.handle(
            "FilterLogEvents",
            json!({
                "logGroupName": "/filter2/group",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(got["events"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_put_log_events_missing_group() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        let now = now_ts();
        let err = block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/ghost",
                "logStreamName": "stream",
                "logEvents": [{ "timestamp": now, "message": "hi" }],
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
    }

    #[test]
    fn test_put_log_events_rejects_too_old_events() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/rej/group", "rej-stream");

        let now = now_ts();
        let too_old = now - (15 * 24 * 60 * 60 * 1000);
        let result = block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/rej/group",
                "logStreamName": "rej-stream",
                "logEvents": [
                    { "timestamp": too_old, "message": "ancient" },
                    { "timestamp": now - 1000, "message": "fresh" },
                ],
            }),
            &ctx,
        ))
        .unwrap();
        let rej = &result["rejectedLogEventsInfo"];
        assert_eq!(rej["tooOldLogEventEndIndex"].as_u64(), Some(0));
    }

    #[test]
    fn test_put_log_events_rejects_too_new_events() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        setup_group_and_stream(&svc, "/rej2/group", "rej2-stream");

        let now = now_ts();
        let too_future = now + (3 * 60 * 60 * 1000);
        let result = block_on(svc.handle(
            "PutLogEvents",
            json!({
                "logGroupName": "/rej2/group",
                "logStreamName": "rej2-stream",
                "logEvents": [
                    { "timestamp": now - 1000, "message": "fresh" },
                    { "timestamp": too_future, "message": "from the future" },
                ],
            }),
            &ctx,
        ))
        .unwrap();
        let rej = &result["rejectedLogEventsInfo"];
        assert_eq!(rej["tooNewLogEventStartIndex"].as_u64(), Some(1));
    }

    #[test]
    fn test_unknown_operation() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("NonExistentOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn test_log_group_arn_format() {
        let svc = CloudWatchLogsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLogGroup",
            json!({ "logGroupName": "/arn/check" }),
            &ctx,
        ))
        .unwrap();

        let result = block_on(svc.handle("DescribeLogGroups", json!({}), &ctx)).unwrap();
        let group = &result["logGroups"].as_array().unwrap()[0];
        let arn = group["arn"].as_str().unwrap();
        assert!(
            arn.starts_with("arn:aws:logs:us-east-1:000000000000:log-group:/arn/check"),
            "arn={arn}"
        );
    }
}
