use crate::chk;
use crate::runner::common::*;

pub async fn test_cloudtrail(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_cloudtrail::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateTrail",
        client.create_trail().name("conf-trail").s3_bucket_name("conf-bucket").send().await,
        verbose
    ));
    results.push(chk!(
        "DescribeTrails",
        client.describe_trails().send().await,
        verbose
    ));
    results.push(chk!(
        "ListTrails",
        client.list_trails().send().await,
        verbose
    ));
    results.push(chk!(
        "GetTrailStatus",
        client.get_trail_status().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "StartLogging",
        client.start_logging().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "StopLogging",
        client.stop_logging().name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "UpdateTrail",
        client.update_trail().name("conf-trail").s3_bucket_name("conf-bucket-2").send().await,
        verbose
    ));
    results.push(chk!(
        "PutEventSelectors",
        client
            .put_event_selectors()
            .trail_name("conf-trail")
            .event_selectors(
                aws_sdk_cloudtrail::types::EventSelector::builder()
                    .read_write_type(aws_sdk_cloudtrail::types::ReadWriteType::All)
                    .include_management_events(true)
                    .build(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "GetEventSelectors",
        client.get_event_selectors().trail_name("conf-trail").send().await,
        verbose
    ));
    results.push(chk!(
        "LookupEvents",
        client.lookup_events().send().await,
        verbose
    ));
    results.push(chk!(
        "DeleteTrail",
        client.delete_trail().name("conf-trail").send().await,
        verbose
    ));

    results
}
