use std::collections::HashMap;
use std::sync::RwLock;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A published SNS message (stored for local-dev debugging/inspection).
/// Fields will be read once cross-service delivery (SQS/Lambda) is implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PublishedMessage {
    pub message_id: String,
    pub topic_arn: String,
    pub message: String,
    pub subject: Option<String>,
    pub message_attributes: HashMap<String, MessageAttribute>,
}

/// A message attribute value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct MessageAttribute {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

/// A single SNS subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub arn: String,
    pub topic_arn: String,
    pub protocol: String,
    pub endpoint: String,
    pub confirmed: bool,
    pub attributes: HashMap<String, String>,
}

/// A single SNS topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Topic {
    pub arn: String,
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    pub is_fifo: bool,
    pub subscription_arns: Vec<String>,
    pub created_at: String,
}

impl Topic {
    pub fn new(
        arn: String,
        name: String,
        is_fifo: bool,
        created_at: String,
        initial_attributes: HashMap<String, String>,
        tags: HashMap<String, String>,
    ) -> Self {
        let mut attributes = default_topic_attributes(is_fifo, &arn);
        for (k, v) in initial_attributes {
            attributes.insert(k, v);
        }
        // Always keep TopicArn consistent
        attributes.insert("TopicArn".to_string(), arn.clone());
        Self {
            arn,
            name,
            attributes,
            tags,
            is_fifo,
            subscription_arns: Vec::new(),
            created_at,
        }
    }
}

/// Serializable snapshot of `SnsState`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SnsStateSnapshot {
    pub topics: Vec<Topic>,
    pub subscriptions: Vec<Subscription>,
}

fn default_topic_attributes(is_fifo: bool, arn: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("TopicArn".to_string(), arn.to_string());
    m.insert(
        "DisplayName".to_string(),
        String::new(),
    );
    m.insert(
        "SubscriptionsConfirmed".to_string(),
        "0".to_string(),
    );
    m.insert(
        "SubscriptionsPending".to_string(),
        "0".to_string(),
    );
    m.insert(
        "SubscriptionsDeleted".to_string(),
        "0".to_string(),
    );
    m.insert(
        "DeliveryPolicy".to_string(),
        String::new(),
    );
    m.insert(
        "EffectiveDeliveryPolicy".to_string(),
        String::new(),
    );
    m.insert(
        "Policy".to_string(),
        String::new(),
    );
    if is_fifo {
        m.insert("FifoTopic".to_string(), "true".to_string());
        m.insert(
            "ContentBasedDeduplication".to_string(),
            "false".to_string(),
        );
    }
    m
}

/// A mobile push platform application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformApplication {
    pub arn: String,
    pub platform: String,
    pub attributes: HashMap<String, String>,
}

/// A push endpoint (device token) registered to a platform application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformEndpoint {
    pub arn: String,
    pub platform_application_arn: String,
    pub token: String,
    pub attributes: HashMap<String, String>,
}

/// Per-account/region SNS state.
#[derive(Debug, Default)]
pub struct SnsState {
    /// TopicArn → Topic
    pub topics: DashMap<String, Topic>,
    /// SubscriptionArn → Subscription
    pub subscriptions: DashMap<String, Subscription>,
    /// SMS attributes (account-level), protected by an RwLock.
    pub sms_attributes: RwLock<HashMap<String, String>>,
    /// PlatformApplicationArn → PlatformApplication
    pub platform_applications: DashMap<String, PlatformApplication>,
    /// EndpointArn → PlatformEndpoint
    pub platform_endpoints: DashMap<String, PlatformEndpoint>,
    /// Opted-in phone numbers (account-level).
    pub opted_in_numbers: RwLock<Vec<String>>,
}

impl SnsState {
    pub fn to_snapshot(&self) -> SnsStateSnapshot {
        SnsStateSnapshot {
            topics: self.topics.iter().map(|e| {
                let t = e.value();
                Topic {
                    arn: t.arn.clone(),
                    name: t.name.clone(),
                    attributes: t.attributes.clone(),
                    tags: t.tags.clone(),
                    is_fifo: t.is_fifo,
                    subscription_arns: t.subscription_arns.clone(),
                    created_at: t.created_at.clone(),
                }
            }).collect(),
            subscriptions: self.subscriptions.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: SnsStateSnapshot) {
        for topic in snapshot.topics {
            self.topics.insert(topic.arn.clone(), topic);
        }
        for sub in snapshot.subscriptions {
            self.subscriptions.insert(sub.arn.clone(), sub);
        }
    }
}
