//! EventBridge notification configuration behavior contract.
//!
//! Enabling EventBridge on a bucket is signalled by an empty
//! `EventBridgeConfiguration` element. This drives it through the S3 SDK and
//! asserts it round-trips on GetBucketNotificationConfiguration.

use aws_sdk_s3::types::{EventBridgeConfiguration, NotificationConfiguration};

#[tokio::test]
async fn eventbridge_configuration_round_trips() {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    let client = aws_sdk_s3::Client::new(&config);

    client
        .create_bucket()
        .bucket("eb-notif")
        .send()
        .await
        .expect("create bucket");

    client
        .put_bucket_notification_configuration()
        .bucket("eb-notif")
        .notification_configuration(
            NotificationConfiguration::builder()
                .event_bridge_configuration(EventBridgeConfiguration::builder().build())
                .build(),
        )
        .send()
        .await
        .expect("put notification configuration");

    let got = client
        .get_bucket_notification_configuration()
        .bucket("eb-notif")
        .send()
        .await
        .expect("get notification configuration");

    assert!(
        got.event_bridge_configuration().is_some(),
        "EventBridge configuration should round-trip"
    );
}
