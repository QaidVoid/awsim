use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use dashmap::DashMap;

/// A message attribute value (type + value).
#[derive(Debug, Clone)]
pub struct MessageAttribute {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

/// A message stored in a queue.
#[derive(Debug, Clone)]
pub struct Message {
    pub message_id: String,
    pub body: String,
    pub md5_of_body: String,
    pub attributes: HashMap<String, String>,
    pub message_attributes: HashMap<String, MessageAttribute>,
    pub sent_at: Instant,
    pub delay_until: Option<Instant>,
    pub sequence_number: Option<String>,
    pub receive_count: u32,
    /// Deduplication ID for FIFO queues.
    pub dedup_id: Option<String>,
    /// Group ID for FIFO queues.
    pub group_id: Option<String>,
}

/// A message that has been received and is now invisible ("inflight").
#[derive(Debug, Clone)]
pub struct InflightMessage {
    pub message: Message,
    pub visible_at: Instant,
    pub receipt_handle: String,
}

/// Per-account/region SQS state.
#[derive(Debug, Default)]
pub struct SqsState {
    /// Queue name → Queue (DashMap for concurrent access)
    pub queues: DashMap<String, Queue>,
}

/// A single SQS queue.
#[derive(Debug)]
pub struct Queue {
    pub name: String,
    pub url: String,
    pub arn: String,
    pub attributes: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    pub messages: VecDeque<Message>,
    /// receipt_handle → inflight message
    pub inflight: HashMap<String, InflightMessage>,
    pub is_fifo: bool,
    pub created_at: String,
    /// FIFO dedup cache: dedup_id → (expiry Instant, message_id)
    pub dedup_cache: HashMap<String, (Instant, String)>,
}

impl Queue {
    pub fn new(
        name: String,
        url: String,
        arn: String,
        is_fifo: bool,
        created_at: String,
        initial_attributes: HashMap<String, String>,
    ) -> Self {
        let mut attributes = default_attributes(is_fifo);
        // Overlay user-supplied attributes
        for (k, v) in initial_attributes {
            attributes.insert(k, v);
        }
        // Ensure QueueArn is always set
        attributes.insert("QueueArn".to_string(), arn.clone());
        Queue {
            name,
            url,
            arn,
            attributes,
            tags: HashMap::new(),
            messages: VecDeque::new(),
            inflight: HashMap::new(),
            is_fifo,
            created_at,
            dedup_cache: HashMap::new(),
        }
    }

    /// Move any messages whose `delay_until` has passed back into the visible pool.
    /// Also expire inflight messages whose visibility timeout has passed.
    pub fn tick(&mut self) {
        let now = Instant::now();

        // Re-enqueue expired inflight messages
        let expired: Vec<String> = self
            .inflight
            .values()
            .filter(|m| m.visible_at <= now)
            .map(|m| m.receipt_handle.clone())
            .collect();

        for rh in expired {
            if let Some(im) = self.inflight.remove(&rh) {
                let mut msg = im.message;
                msg.receive_count += 1;
                // Re-insert at front so it can be received again quickly
                self.messages.push_front(msg);
            }
        }

        // Purge stale FIFO dedup cache entries (5-minute window)
        let five_min = std::time::Duration::from_secs(300);
        self.dedup_cache
            .retain(|_, (expiry, _)| now < *expiry + five_min);
    }

    /// Number of visible messages (not delayed, not inflight).
    pub fn approximate_number_of_messages(&self) -> usize {
        let now = Instant::now();
        self.messages
            .iter()
            .filter(|m| m.delay_until.map_or(true, |d| d <= now))
            .count()
    }

    /// Number of delayed messages.
    pub fn approximate_number_of_messages_delayed(&self) -> usize {
        let now = Instant::now();
        self.messages
            .iter()
            .filter(|m| m.delay_until.map_or(false, |d| d > now))
            .count()
    }

    /// Number of inflight messages.
    pub fn approximate_number_of_messages_not_visible(&self) -> usize {
        self.inflight.len()
    }

    /// Default visibility timeout (seconds), parsed from attributes.
    pub fn visibility_timeout_secs(&self) -> u64 {
        self.attributes
            .get("VisibilityTimeout")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30)
    }

    /// Default delay seconds, parsed from attributes.
    pub fn delay_seconds(&self) -> u64 {
        self.attributes
            .get("DelaySeconds")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}

fn default_attributes(is_fifo: bool) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("VisibilityTimeout".to_string(), "30".to_string());
    m.insert("MaximumMessageSize".to_string(), "262144".to_string());
    m.insert("MessageRetentionPeriod".to_string(), "345600".to_string());
    m.insert("DelaySeconds".to_string(), "0".to_string());
    m.insert("ReceiveMessageWaitTimeSeconds".to_string(), "0".to_string());
    m.insert("ApproximateNumberOfMessages".to_string(), "0".to_string());
    m.insert(
        "ApproximateNumberOfMessagesNotVisible".to_string(),
        "0".to_string(),
    );
    m.insert(
        "ApproximateNumberOfMessagesDelayed".to_string(),
        "0".to_string(),
    );
    if is_fifo {
        m.insert("FifoQueue".to_string(), "true".to_string());
        m.insert(
            "ContentBasedDeduplication".to_string(),
            "false".to_string(),
        );
        m.insert(
            "DeduplicationScope".to_string(),
            "queue".to_string(),
        );
        m.insert(
            "FifoThroughputLimit".to_string(),
            "perQueue".to_string(),
        );
    }
    m
}
