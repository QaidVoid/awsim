use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{publish, subscriptions, tags, topics};
use crate::state::SnsState;

/// The SNS service handler.
pub struct SnsService {
    store: AccountRegionStore<SnsState>,
}

impl SnsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for SnsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for SnsService {
    fn service_name(&self) -> &str {
        "sns"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "SNS operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            // Topics
            "CreateTopic" => topics::create_topic(&state, &input, ctx),
            "DeleteTopic" => topics::delete_topic(&state, &input, ctx),
            "ListTopics" => topics::list_topics(&state, &input, ctx),
            "GetTopicAttributes" => topics::get_topic_attributes(&state, &input, ctx),
            "SetTopicAttributes" => topics::set_topic_attributes(&state, &input, ctx),

            // Tags
            "TagResource" => tags::tag_resource(&state, &input, ctx),
            "UntagResource" => tags::untag_resource(&state, &input, ctx),
            "ListTagsForResource" => tags::list_tags_for_resource(&state, &input, ctx),

            // Subscriptions
            "Subscribe" => subscriptions::subscribe(&state, &input, ctx),
            "Unsubscribe" => subscriptions::unsubscribe(&state, &input, ctx),
            "ListSubscriptions" => subscriptions::list_subscriptions(&state, &input, ctx),
            "ListSubscriptionsByTopic" => {
                subscriptions::list_subscriptions_by_topic(&state, &input, ctx)
            }
            "GetSubscriptionAttributes" => {
                subscriptions::get_subscription_attributes(&state, &input, ctx)
            }
            "SetSubscriptionAttributes" => {
                subscriptions::set_subscription_attributes(&state, &input, ctx)
            }
            "ConfirmSubscription" => subscriptions::confirm_subscription(&state, &input, ctx),

            // Publishing
            "Publish" => publish::publish(&state, &input, ctx),
            "PublishBatch" => publish::publish_batch(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
