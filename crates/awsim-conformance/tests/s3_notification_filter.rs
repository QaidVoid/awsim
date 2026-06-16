//! Notification object key filter behavior contract.
//!
//! Drives PutBucketNotificationConfiguration with an `S3Key` prefix and
//! suffix filter through the S3 SDK and asserts the filter round-trips on
//! GetBucketNotificationConfiguration, confirming the wire format is parsed
//! and re-emitted correctly.

use aws_sdk_s3::types::{
    Event, FilterRule, FilterRuleName, NotificationConfiguration, QueueConfiguration, S3KeyFilter,
};

#[tokio::test]
async fn notification_key_filter_round_trips() {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    let client = aws_sdk_s3::Client::new(&config);

    client
        .create_bucket()
        .bucket("notif-filter")
        .send()
        .await
        .expect("create bucket");

    let key_filter = S3KeyFilter::builder()
        .filter_rules(
            FilterRule::builder()
                .name(FilterRuleName::Prefix)
                .value("uploads/")
                .build(),
        )
        .filter_rules(
            FilterRule::builder()
                .name(FilterRuleName::Suffix)
                .value(".jpg")
                .build(),
        )
        .build();
    let queue_config = QueueConfiguration::builder()
        .queue_arn("arn:aws:sqs:us-east-1:000000000000:events")
        .events(Event::S3ObjectCreated)
        .filter(
            aws_sdk_s3::types::NotificationConfigurationFilter::builder()
                .key(key_filter)
                .build(),
        )
        .build()
        .expect("build queue config");

    client
        .put_bucket_notification_configuration()
        .bucket("notif-filter")
        .notification_configuration(
            NotificationConfiguration::builder()
                .queue_configurations(queue_config)
                .build(),
        )
        .send()
        .await
        .expect("put notification configuration");

    let got = client
        .get_bucket_notification_configuration()
        .bucket("notif-filter")
        .send()
        .await
        .expect("get notification configuration");

    let queue = &got.queue_configurations()[0];
    let rules = queue
        .filter()
        .expect("filter present")
        .key()
        .expect("s3 key filter present")
        .filter_rules();
    let prefix = rules
        .iter()
        .find(|r| r.name() == Some(&FilterRuleName::Prefix))
        .and_then(|r| r.value());
    let suffix = rules
        .iter()
        .find(|r| r.name() == Some(&FilterRuleName::Suffix))
        .and_then(|r| r.value());
    assert_eq!(prefix, Some("uploads/"));
    assert_eq!(suffix, Some(".jpg"));
}
