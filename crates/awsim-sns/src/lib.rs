pub mod authz;
pub mod filter;
mod handler;
mod operations;
pub mod state;

pub use authz::SnsResourcePolicyLookup;
pub use handler::SnsService;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::SnsService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("sns", "us-east-1")
    }

    /// Minimal blocking executor for async tests.
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
    // Topic operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_topic_basic() {
        let svc = SnsService::new();
        let ctx = ctx();
        let result =
            block_on(svc.handle("CreateTopic", json!({ "Name": "my-topic" }), &ctx)).unwrap();
        let arn = result["TopicArn"].as_str().unwrap();
        assert!(
            arn.starts_with("arn:aws:sns:us-east-1:000000000000:my-topic"),
            "arn={arn}"
        );
    }

    #[test]
    fn test_create_topic_fifo() {
        let svc = SnsService::new();
        let ctx = ctx();
        let result =
            block_on(svc.handle("CreateTopic", json!({ "Name": "my-topic.fifo" }), &ctx)).unwrap();
        let arn = result["TopicArn"].as_str().unwrap();
        assert!(arn.ends_with(".fifo"), "arn={arn}");
    }

    #[test]
    fn test_create_topic_idempotent() {
        let svc = SnsService::new();
        let ctx = ctx();
        let r1 = block_on(svc.handle("CreateTopic", json!({ "Name": "idempotent-topic" }), &ctx))
            .unwrap();
        let r2 = block_on(svc.handle("CreateTopic", json!({ "Name": "idempotent-topic" }), &ctx))
            .unwrap();
        assert_eq!(r1["TopicArn"], r2["TopicArn"]);
    }

    #[test]
    fn test_create_topic_missing_name() {
        let svc = SnsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("CreateTopic", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_list_topics_empty() {
        let svc = SnsService::new();
        let ctx = ctx();
        let result = block_on(svc.handle("ListTopics", json!({}), &ctx)).unwrap();
        assert_eq!(result["Topics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_list_topics_after_create() {
        let svc = SnsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateTopic", json!({ "Name": "topic-a" }), &ctx)).unwrap();
        block_on(svc.handle("CreateTopic", json!({ "Name": "topic-b" }), &ctx)).unwrap();
        let result = block_on(svc.handle("ListTopics", json!({}), &ctx)).unwrap();
        assert_eq!(result["Topics"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_get_topic_attributes() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "attr-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();
        let result =
            block_on(svc.handle("GetTopicAttributes", json!({ "TopicArn": arn }), &ctx)).unwrap();
        assert!(result["Attributes"]["TopicArn"].as_str().is_some());
    }

    #[test]
    fn test_set_topic_attributes() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "settable-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "SetTopicAttributes",
            json!({
                "TopicArn": arn,
                "AttributeName": "DisplayName",
                "AttributeValue": "My Topic",
            }),
            &ctx,
        ))
        .unwrap();

        let attrs =
            block_on(svc.handle("GetTopicAttributes", json!({ "TopicArn": arn }), &ctx)).unwrap();
        assert_eq!(
            attrs["Attributes"]["DisplayName"].as_str().unwrap(),
            "My Topic"
        );
    }

    #[test]
    fn test_delete_topic() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "delete-me" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle("DeleteTopic", json!({ "TopicArn": arn }), &ctx)).unwrap();

        let list = block_on(svc.handle("ListTopics", json!({}), &ctx)).unwrap();
        assert_eq!(list["Topics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_topic() {
        let svc = SnsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "DeleteTopic",
            json!({ "TopicArn": "arn:aws:sns:us-east-1:000000000000:ghost" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "NotFound");
    }

    // -----------------------------------------------------------------------
    // Tags
    // -----------------------------------------------------------------------

    #[test]
    fn test_tag_and_list_tags() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "tagged-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceArn": arn,
                "Tags": [
                    { "Key": "env", "Value": "test" },
                    { "Key": "team", "Value": "infra" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle("ListTagsForResource", json!({ "ResourceArn": arn }), &ctx))
            .unwrap();
        let tag_arr = tags["Tags"].as_array().unwrap();
        assert_eq!(tag_arr.len(), 2);
    }

    #[test]
    fn test_untag_resource() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "untag-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "TagResource",
            json!({
                "ResourceArn": arn,
                "Tags": [{ "Key": "remove-me", "Value": "yes" }],
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "UntagResource",
            json!({ "ResourceArn": arn, "TagKeys": ["remove-me"] }),
            &ctx,
        ))
        .unwrap();

        let tags = block_on(svc.handle("ListTagsForResource", json!({ "ResourceArn": arn }), &ctx))
            .unwrap();
        assert_eq!(tags["Tags"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Subscriptions
    // -----------------------------------------------------------------------

    #[test]
    fn test_subscribe_and_list() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "sub-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let sub_result = block_on(svc.handle(
            "Subscribe",
            json!({
                "TopicArn": arn,
                "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:my-queue",
            }),
            &ctx,
        ))
        .unwrap();
        let sub_arn = sub_result["SubscriptionArn"].as_str().unwrap();
        assert!(sub_arn.contains(":sub-topic:"), "sub_arn={sub_arn}");

        let list = block_on(svc.handle("ListSubscriptions", json!({}), &ctx)).unwrap();
        assert_eq!(list["Subscriptions"].as_array().unwrap().len(), 1);

        let by_topic =
            block_on(svc.handle("ListSubscriptionsByTopic", json!({ "TopicArn": arn }), &ctx))
                .unwrap();
        assert_eq!(by_topic["Subscriptions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_invalid_protocol() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "proto-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let err = block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "ftp" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_unsubscribe() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "unsub-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        // Use sqs protocol so Subscribe returns the real SubscriptionArn
        // without needing the confirmation-token round-trip.
        let sub = block_on(svc.handle(
            "Subscribe",
            json!({
                "TopicArn": arn,
                "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q",
            }),
            &ctx,
        ))
        .unwrap();
        let sub_arn = sub["SubscriptionArn"].as_str().unwrap();
        assert!(sub_arn.starts_with("arn:aws:sns:"));

        block_on(svc.handle("Unsubscribe", json!({ "SubscriptionArn": sub_arn }), &ctx)).unwrap();

        let list = block_on(svc.handle("ListSubscriptions", json!({}), &ctx)).unwrap();
        assert_eq!(list["Subscriptions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_get_subscription_attributes() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "getattr-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let sub = block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q" }),
            &ctx,
        ))
        .unwrap();
        let sub_arn = sub["SubscriptionArn"].as_str().unwrap();

        let attrs = block_on(svc.handle(
            "GetSubscriptionAttributes",
            json!({ "SubscriptionArn": sub_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(attrs["Attributes"]["Protocol"].as_str().unwrap(), "sqs");
    }

    #[test]
    fn test_subscribe_rejects_invalid_filter_policy_json() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "fp-bad-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let err = block_on(svc.handle(
            "Subscribe",
            json!({
                "TopicArn": arn,
                "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q",
                "Attributes": { "FilterPolicy": "not-json" },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.contains("FilterPolicy"));
    }

    #[test]
    fn test_subscribe_rejects_filter_policy_with_bad_operator() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "fp-bad2-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        // numeric without value pairs is malformed.
        let bad = json!({ "count": [{ "numeric": [">"] }] }).to_string();
        let err = block_on(svc.handle(
            "Subscribe",
            json!({
                "TopicArn": arn,
                "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q",
                "Attributes": { "FilterPolicy": bad },
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_set_subscription_attributes_rejects_invalid_filter_policy() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "fp-set-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();
        let sub = block_on(svc.handle(
            "Subscribe",
            json!({
                "TopicArn": arn,
                "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q",
            }),
            &ctx,
        ))
        .unwrap();
        let sub_arn = sub["SubscriptionArn"].as_str().unwrap();

        let err = block_on(svc.handle(
            "SetSubscriptionAttributes",
            json!({
                "SubscriptionArn": sub_arn,
                "AttributeName": "FilterPolicy",
                "AttributeValue": "{\"k\": \"not-an-array\"}",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_subscribe_returns_pending_for_http_protocol() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "pending-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let sub = block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "https", "Endpoint": "https://example.com" }),
            &ctx,
        ))
        .unwrap();
        // AWS returns the literal "pending confirmation" placeholder for
        // protocols that need token round-trip, not the real ARN.
        assert_eq!(
            sub["SubscriptionArn"].as_str(),
            Some("pending confirmation")
        );
    }

    #[test]
    fn test_confirm_subscription_rejects_invalid_token() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created = block_on(svc.handle(
            "CreateTopic",
            json!({ "Name": "confirm-bad-token-topic" }),
            &ctx,
        ))
        .unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "email", "Endpoint": "test@example.com" }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "ConfirmSubscription",
            json!({ "TopicArn": arn, "Token": "definitely-not-the-token" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_confirm_subscription_succeeds_with_valid_token() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "confirm-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "email", "Endpoint": "test@example.com" }),
            &ctx,
        ))
        .unwrap();

        // Pull the actual generated token out of internal state — AWS would
        // have delivered this via the SubscriptionConfirmation control
        // message to the endpoint, which we don't simulate.
        let token = {
            let state = svc.store().get("000000000000", "us-east-1");
            let entry = state
                .subscriptions
                .iter()
                .find(|s| s.topic_arn == arn)
                .expect("pending sub present");
            entry
                .attributes
                .get("_AwsimConfirmationToken")
                .cloned()
                .expect("token stored on pending sub")
        };

        let confirmed = block_on(svc.handle(
            "ConfirmSubscription",
            json!({ "TopicArn": arn, "Token": token }),
            &ctx,
        ))
        .unwrap();
        assert!(
            confirmed["SubscriptionArn"]
                .as_str()
                .unwrap()
                .starts_with("arn:aws:sns:")
        );
    }

    // -----------------------------------------------------------------------
    // Publishing
    // -----------------------------------------------------------------------

    #[test]
    fn test_publish_success() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "pub-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let result = block_on(svc.handle(
            "Publish",
            json!({ "TopicArn": arn, "Message": "Hello, SNS!" }),
            &ctx,
        ))
        .unwrap();
        assert!(result["MessageId"].as_str().is_some());
    }

    #[test]
    fn test_publish_to_nonexistent_topic() {
        let svc = SnsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "Publish",
            json!({
                "TopicArn": "arn:aws:sns:us-east-1:000000000000:ghost",
                "Message": "oops"
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "NotFound");
    }

    #[test]
    fn test_publish_missing_message() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "no-msg-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let err = block_on(svc.handle("Publish", json!({ "TopicArn": arn }), &ctx)).unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_publish_message_structure_json_requires_default_key() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "json-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        // Missing "default" key — must reject.
        let err = block_on(svc.handle(
            "Publish",
            json!({
                "TopicArn": arn,
                "Message": json!({ "sqs": "for sqs" }).to_string(),
                "MessageStructure": "json",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
        assert!(err.message.to_lowercase().contains("default"));
    }

    #[test]
    fn test_publish_message_structure_json_rejects_invalid_json() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "json-topic2" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let err = block_on(svc.handle(
            "Publish",
            json!({
                "TopicArn": arn,
                "Message": "not json",
                "MessageStructure": "json",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameter");
    }

    #[test]
    fn test_publish_message_structure_json_with_default_succeeds() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "json-topic3" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let result = block_on(svc.handle(
            "Publish",
            json!({
                "TopicArn": arn,
                "Message": json!({ "default": "fallback", "sqs": "for sqs" }).to_string(),
                "MessageStructure": "json",
            }),
            &ctx,
        ))
        .unwrap();
        assert!(result["MessageId"].as_str().is_some());
    }

    #[test]
    fn test_publish_batch_success() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "batch-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let result = block_on(svc.handle(
            "PublishBatch",
            json!({
                "TopicArn": arn,
                "PublishBatchRequestEntries": [
                    { "Id": "1", "Message": "msg-one" },
                    { "Id": "2", "Message": "msg-two" },
                ],
            }),
            &ctx,
        ))
        .unwrap();

        let successful = result["Successful"].as_array().unwrap();
        let failed = result["Failed"].as_array().unwrap();
        assert_eq!(successful.len(), 2);
        assert_eq!(failed.len(), 0);
        assert!(successful[0]["MessageId"].as_str().is_some());
    }

    #[test]
    fn test_publish_batch_too_many_entries() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "bigbatch-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        let entries: Vec<_> = (0..11)
            .map(|i| json!({ "Id": i.to_string(), "Message": "msg" }))
            .collect();

        let err = block_on(svc.handle(
            "PublishBatch",
            json!({ "TopicArn": arn, "PublishBatchRequestEntries": entries }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "TooManyEntriesInBatchRequest");
    }

    #[test]
    fn test_unknown_operation() {
        let svc = SnsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("NonExistentOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    #[test]
    fn test_delete_topic_removes_subscriptions() {
        let svc = SnsService::new();
        let ctx = ctx();
        let created =
            block_on(svc.handle("CreateTopic", json!({ "Name": "cleanup-topic" }), &ctx)).unwrap();
        let arn = created["TopicArn"].as_str().unwrap();

        block_on(svc.handle(
            "Subscribe",
            json!({ "TopicArn": arn, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q" }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle("DeleteTopic", json!({ "TopicArn": arn }), &ctx)).unwrap();

        let subs = block_on(svc.handle("ListSubscriptions", json!({}), &ctx)).unwrap();
        assert_eq!(subs["Subscriptions"].as_array().unwrap().len(), 0);
    }
}
