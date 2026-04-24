use crate::chk;
use crate::runner::common::*;

pub async fn test_sns(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sns::Client::new(&config);
    let mut results = Vec::new();

    // CreateTopic
    let create_r = client
        .create_topic()
        .name("conformance-topic")
        .send()
        .await;
    let topic_arn = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.topic_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:sns:us-east-1:000000000000:conformance-topic".to_string()
        });
    results.push(chk!("CreateTopic", create_r, verbose));

    // ListTopics
    results.push(chk!(
        "ListTopics",
        client.list_topics().send().await,
        verbose
    ));

    // GetTopicAttributes
    results.push(chk!(
        "GetTopicAttributes",
        client
            .get_topic_attributes()
            .topic_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // Publish
    results.push(chk!(
        "Publish",
        client
            .publish()
            .topic_arn(&topic_arn)
            .message("conformance test message")
            .send()
            .await,
        verbose
    ));

    // Subscribe (email — no confirmation needed in sim)
    let sub_r = client
        .subscribe()
        .topic_arn(&topic_arn)
        .protocol("email")
        .endpoint("test@example.com")
        .send()
        .await;
    let subscription_arn = sub_r
        .as_ref()
        .ok()
        .and_then(|r| r.subscription_arn.clone());
    results.push(chk!("Subscribe", sub_r, verbose));

    // ListSubscriptions
    results.push(chk!(
        "ListSubscriptions",
        client.list_subscriptions().send().await,
        verbose
    ));

    // ListSubscriptionsByTopic
    results.push(chk!(
        "ListSubscriptionsByTopic",
        client
            .list_subscriptions_by_topic()
            .topic_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // SetTopicAttributes
    results.push(chk!(
        "SetTopicAttributes",
        client
            .set_topic_attributes()
            .topic_arn(&topic_arn)
            .attribute_name("DisplayName")
            .attribute_value("Conformance Topic")
            .send()
            .await,
        verbose
    ));

    // GetSubscriptionAttributes (if we got a subscription ARN)
    if let Some(ref sub_arn) = subscription_arn {
        results.push(chk!(
            "GetSubscriptionAttributes",
            client
                .get_subscription_attributes()
                .subscription_arn(sub_arn)
                .send()
                .await,
            verbose
        ));

        // SetSubscriptionAttributes
        results.push(chk!(
            "SetSubscriptionAttributes",
            client
                .set_subscription_attributes()
                .subscription_arn(sub_arn)
                .attribute_name("RawMessageDelivery")
                .attribute_value("true")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetSubscriptionAttributes".to_string()));
        results.push(OpResult::Skipped("SetSubscriptionAttributes".to_string()));
    }

    // TagResource (SNS)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&topic_arn)
            .tags(
                aws_sdk_sns::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SNS)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource (SNS)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&topic_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // PublishBatch
    results.push(chk!(
        "PublishBatch",
        client
            .publish_batch()
            .topic_arn(&topic_arn)
            .publish_batch_request_entries(
                aws_sdk_sns::types::PublishBatchRequestEntry::builder()
                    .id("msg-1")
                    .message("batch conformance message")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // Unsubscribe (if we got a subscription ARN)
    if let Some(sub_arn) = subscription_arn {
        results.push(chk!(
            "Unsubscribe",
            client.unsubscribe().subscription_arn(sub_arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("Unsubscribe".to_string()));
    }

    // AddPermission (SNS)
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .topic_arn(&topic_arn)
            .label("conformance-perm")
            .aws_account_id("000000000000")
            .action_name("Publish")
            .send()
            .await,
        verbose
    ));

    // RemovePermission (SNS)
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .topic_arn(&topic_arn)
            .label("conformance-perm")
            .send()
            .await,
        verbose
    ));

    // CheckIfPhoneNumberIsOptedOut
    results.push(chk!(
        "CheckIfPhoneNumberIsOptedOut",
        client
            .check_if_phone_number_is_opted_out()
            .phone_number("+15005550006")
            .send()
            .await,
        verbose
    ));

    // ListPhoneNumbersOptedOut
    results.push(chk!(
        "ListPhoneNumbersOptedOut",
        client.list_phone_numbers_opted_out().send().await,
        verbose
    ));

    // GetSMSAttributes
    results.push(chk!(
        "GetSMSAttributes",
        client.get_sms_attributes().send().await,
        verbose
    ));

    // SetSMSAttributes
    results.push(chk!(
        "SetSMSAttributes",
        client
            .set_sms_attributes()
            .attributes("DefaultSMSType", "Transactional")
            .send()
            .await,
        verbose
    ));

    // OptInPhoneNumber
    results.push(chk!(
        "OptInPhoneNumber",
        client
            .opt_in_phone_number()
            .phone_number("+15005550006")
            .send()
            .await,
        verbose
    ));

    // ListOriginationNumbers
    results.push(chk!(
        "ListOriginationNumbers",
        client.list_origination_numbers().send().await,
        verbose
    ));

    // CreatePlatformApplication
    let platform_app_r = client
        .create_platform_application()
        .name("conformance-app")
        .platform("GCM")
        .attributes("PlatformCredential", "fake-server-key")
        .send()
        .await;
    let platform_app_arn = platform_app_r
        .as_ref()
        .ok()
        .and_then(|r| r.platform_application_arn.clone());
    results.push(chk!("CreatePlatformApplication", platform_app_r, verbose));

    // ListPlatformApplications
    results.push(chk!(
        "ListPlatformApplications",
        client.list_platform_applications().send().await,
        verbose
    ));

    if let Some(ref app_arn) = platform_app_arn {
        // GetPlatformApplicationAttributes
        results.push(chk!(
            "GetPlatformApplicationAttributes",
            client
                .get_platform_application_attributes()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));

        // SetPlatformApplicationAttributes
        results.push(chk!(
            "SetPlatformApplicationAttributes",
            client
                .set_platform_application_attributes()
                .platform_application_arn(app_arn)
                .attributes("EventDeliveryFailure", "arn:aws:sns:us-east-1:000000000000:conformance-topic")
                .send()
                .await,
            verbose
        ));

        // CreatePlatformEndpoint
        let endpoint_r = client
            .create_platform_endpoint()
            .platform_application_arn(app_arn)
            .token("fake-device-token-conformance")
            .send()
            .await;
        let endpoint_arn = endpoint_r
            .as_ref()
            .ok()
            .and_then(|r| r.endpoint_arn.clone());
        results.push(chk!("CreatePlatformEndpoint", endpoint_r, verbose));

        // ListEndpointsByPlatformApplication
        results.push(chk!(
            "ListEndpointsByPlatformApplication",
            client
                .list_endpoints_by_platform_application()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));

        if let Some(ref ep_arn) = endpoint_arn {
            // GetEndpointAttributes
            results.push(chk!(
                "GetEndpointAttributes",
                client.get_endpoint_attributes().endpoint_arn(ep_arn).send().await,
                verbose
            ));

            // SetEndpointAttributes
            results.push(chk!(
                "SetEndpointAttributes",
                client
                    .set_endpoint_attributes()
                    .endpoint_arn(ep_arn)
                    .attributes("Enabled", "true")
                    .send()
                    .await,
                verbose
            ));

            // DeleteEndpoint
            results.push(chk!(
                "DeleteEndpoint",
                client.delete_endpoint().endpoint_arn(ep_arn).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetEndpointAttributes".to_string()));
            results.push(OpResult::Skipped("SetEndpointAttributes".to_string()));
            results.push(OpResult::Skipped("DeleteEndpoint".to_string()));
        }

        // DeletePlatformApplication
        results.push(chk!(
            "DeletePlatformApplication",
            client
                .delete_platform_application()
                .platform_application_arn(app_arn)
                .send()
                .await,
            verbose
        ));
    } else {
        for op in &[
            "GetPlatformApplicationAttributes",
            "SetPlatformApplicationAttributes",
            "CreatePlatformEndpoint",
            "ListEndpointsByPlatformApplication",
            "GetEndpointAttributes",
            "SetEndpointAttributes",
            "DeleteEndpoint",
            "DeletePlatformApplication",
        ] {
            results.push(OpResult::Skipped(op.to_string()));
        }
    }

    // GetSMSSandboxAccountStatus
    results.push(chk!(
        "GetSMSSandboxAccountStatus",
        client.get_sms_sandbox_account_status().send().await,
        verbose
    ));

    // ListSMSSandboxPhoneNumbers
    results.push(chk!(
        "ListSMSSandboxPhoneNumbers",
        client.list_sms_sandbox_phone_numbers().send().await,
        verbose
    ));

    // PutDataProtectionPolicy
    let dp_policy = r#"{"Name":"conformance","Version":"2021-06-01","Statement":[]}"#;
    results.push(chk!(
        "PutDataProtectionPolicy",
        client
            .put_data_protection_policy()
            .resource_arn(&topic_arn)
            .data_protection_policy(dp_policy)
            .send()
            .await,
        verbose
    ));

    // GetDataProtectionPolicy
    results.push(chk!(
        "GetDataProtectionPolicy",
        client
            .get_data_protection_policy()
            .resource_arn(&topic_arn)
            .send()
            .await,
        verbose
    ));

    // DeleteTopic
    results.push(chk!(
        "DeleteTopic",
        client.delete_topic().topic_arn(&topic_arn).send().await,
        verbose
    ));

    results
}
