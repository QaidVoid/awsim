use crate::chk;
use crate::runner::common::*;

pub async fn test_cloudwatch_logs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);
    let mut results = Vec::new();

    // CreateLogGroup
    results.push(chk!(
        "CreateLogGroup",
        client
            .create_log_group()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // DescribeLogGroups
    results.push(chk!(
        "DescribeLogGroups",
        client
            .describe_log_groups()
            .log_group_name_prefix("/conformance")
            .send()
            .await,
        verbose
    ));

    // CreateLogStream
    results.push(chk!(
        "CreateLogStream",
        client
            .create_log_stream()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DescribeLogStreams
    results.push(chk!(
        "DescribeLogStreams",
        client
            .describe_log_streams()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // PutLogEvents
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    results.push(chk!(
        "PutLogEvents",
        client
            .put_log_events()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .log_events(
                aws_sdk_cloudwatchlogs::types::InputLogEvent::builder()
                    .timestamp(now_ms)
                    .message("conformance test log event")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetLogEvents
    results.push(chk!(
        "GetLogEvents",
        client
            .get_log_events()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // FilterLogEvents
    results.push(chk!(
        "FilterLogEvents",
        client
            .filter_log_events()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // PutSubscriptionFilter
    results.push(chk!(
        "PutSubscriptionFilter",
        client
            .put_subscription_filter()
            .log_group_name("/conformance/logs")
            .filter_name("conformance-filter")
            .filter_pattern("")
            .destination_arn(
                "arn:aws:lambda:us-east-1:000000000000:function:conformance-fn",
            )
            .send()
            .await,
        verbose
    ));

    // DescribeSubscriptionFilters
    results.push(chk!(
        "DescribeSubscriptionFilters",
        client
            .describe_subscription_filters()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    // DeleteSubscriptionFilter
    results.push(chk!(
        "DeleteSubscriptionFilter",
        client
            .delete_subscription_filter()
            .log_group_name("/conformance/logs")
            .filter_name("conformance-filter")
            .send()
            .await,
        verbose
    ));

    // DeleteLogStream
    results.push(chk!(
        "DeleteLogStream",
        client
            .delete_log_stream()
            .log_group_name("/conformance/logs")
            .log_stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DeleteLogGroup
    results.push(chk!(
        "DeleteLogGroup",
        client
            .delete_log_group()
            .log_group_name("/conformance/logs")
            .send()
            .await,
        verbose
    ));

    results
}
