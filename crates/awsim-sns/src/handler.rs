use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{permissions, platform, publish, sms, subscriptions, tags, topics};
use crate::state::{SnsState, SnsStateSnapshot};

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

            // SMS
            "CheckIfPhoneNumberIsOptedOut" => {
                sms::check_if_phone_number_is_opted_out(&state, &input, ctx)
            }
            "ListPhoneNumbersOptedOut" => {
                sms::list_phone_numbers_opted_out(&state, &input, ctx)
            }
            "GetSMSAttributes" => sms::get_sms_attributes(&state, &input, ctx),
            "SetSMSAttributes" => sms::set_sms_attributes(&state, &input, ctx),
            "CreateSMSSandboxPhoneNumber" => sms::create_sms_sandbox_phone_number(&state, &input, ctx),
            "DeleteSMSSandboxPhoneNumber" => sms::delete_sms_sandbox_phone_number(&state, &input, ctx),
            "VerifySMSSandboxPhoneNumber" => sms::verify_sms_sandbox_phone_number(&state, &input, ctx),
            "ListSMSSandboxPhoneNumbers" => sms::list_sms_sandbox_phone_numbers(&state, &input, ctx),
            "GetSMSSandboxAccountStatus" => sms::get_sms_sandbox_account_status(&state, &input, ctx),
            "GetDataProtectionPolicy" => sms::get_data_protection_policy(&state, &input, ctx),
            "PutDataProtectionPolicy" => sms::put_data_protection_policy(&state, &input, ctx),

            // Platform applications
            "CreatePlatformApplication" => platform::create_platform_application(&state, &input, ctx),
            "DeletePlatformApplication" => platform::delete_platform_application(&state, &input, ctx),
            "ListPlatformApplications" => platform::list_platform_applications(&state, &input, ctx),
            "GetPlatformApplicationAttributes" => platform::get_platform_application_attributes(&state, &input, ctx),
            "SetPlatformApplicationAttributes" => platform::set_platform_application_attributes(&state, &input, ctx),

            // Push endpoints
            "CreatePlatformEndpoint" => platform::create_platform_endpoint(&state, &input, ctx),
            "DeleteEndpoint" => platform::delete_endpoint(&state, &input, ctx),
            "ListEndpointsByPlatformApplication" => platform::list_endpoints_by_platform_application(&state, &input, ctx),
            "GetEndpointAttributes" => platform::get_endpoint_attributes(&state, &input, ctx),
            "SetEndpointAttributes" => platform::set_endpoint_attributes(&state, &input, ctx),

            // Phone numbers
            "OptInPhoneNumber" => platform::opt_in_phone_number(&state, &input, ctx),
            "ListOriginationNumbers" => platform::list_origination_numbers(&state, &input, ctx),

            // Topic permissions
            "AddPermission" => permissions::add_permission(&state, &input, ctx),
            "RemovePermission" => permissions::remove_permission(&state, &input, ctx),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all: Vec<SnsStateSnapshot> = Vec::new();

        for ((_account, _region), state) in self.store.iter_all() {
            all.push(state.to_snapshot());
        }

        // Combine all per-account-region snapshots into one flat structure
        let combined = SnsStateSnapshot {
            topics: all.iter().flat_map(|s| s.topics.iter().cloned()).collect(),
            subscriptions: all.iter().flat_map(|s| s.subscriptions.iter().cloned()).collect(),
        };

        serde_json::to_vec(&combined).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: SnsStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        // Group by account+region derived from topic ARN.
        // Topic ARN format: arn:aws:sns:{region}:{account}:{name}
        use std::collections::HashMap;
        let mut by_acct_region: HashMap<(String, String), SnsStateSnapshot> = HashMap::new();

        for topic in snapshot.topics {
            let parts: Vec<&str> = topic.arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };
            by_acct_region
                .entry((account, region))
                .or_insert_with(|| SnsStateSnapshot { topics: vec![], subscriptions: vec![] })
                .topics
                .push(topic);
        }

        for sub in snapshot.subscriptions {
            let parts: Vec<&str> = sub.topic_arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };
            by_acct_region
                .entry((account, region))
                .or_insert_with(|| SnsStateSnapshot { topics: vec![], subscriptions: vec![] })
                .subscriptions
                .push(sub);
        }

        for ((account, region), snap) in by_acct_region {
            let state = self.store.get(&account, &region);
            state.restore_from_snapshot(snap);
        }

        Ok(())
    }
}
