use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A message attribute value (type + value).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttribute {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

/// A message stored in a queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: String,
    pub body: String,
    pub md5_of_body: String,
    pub attributes: HashMap<String, String>,
    pub message_attributes: HashMap<String, MessageAttribute>,
    /// Wall-clock timestamp (seconds since Unix epoch) — replaces `Instant`.
    pub sent_at_secs: u64,
    /// Epoch seconds when the message becomes visible; `None` = immediately.
    pub delay_until_secs: Option<u64>,
    pub sequence_number: Option<String>,
    pub receive_count: u32,
    /// Deduplication ID for FIFO queues.
    pub dedup_id: Option<String>,
    /// Group ID for FIFO queues.
    pub group_id: Option<String>,
    /// Non-serialized original `Instant` used for in-process delay calculations.
    /// Re-derived from `delay_until_secs` on restore.
    #[serde(skip)]
    pub sent_at: Option<Instant>,
    #[serde(skip)]
    pub delay_until: Option<Instant>,
}

impl Message {
    /// Reconstruct `Instant`-based fields from the persisted epoch-second fields.
    pub fn reinit_instants(&mut self) {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let now_instant = Instant::now();

        // Reconstruct sent_at as an Instant relative to now
        let sent_offset = now_epoch.saturating_sub(self.sent_at_secs);
        self.sent_at = Some(now_instant - Duration::from_secs(sent_offset));

        // Reconstruct delay_until
        self.delay_until = self.delay_until_secs.map(|due| {
            if due > now_epoch {
                now_instant + Duration::from_secs(due - now_epoch)
            } else {
                // Already past — make it immediately visible
                now_instant
            }
        });
    }
}

/// A message that has been received and is now invisible ("inflight").
/// Inflight messages are intentionally not persisted — on restore they are
/// treated as if their visibility timeout expired (i.e., returned to the queue).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflightMessage {
    pub message: Message,
    /// Epoch seconds when the message becomes visible again.
    pub visible_at_secs: u64,
    pub receipt_handle: String,
    #[serde(skip)]
    pub visible_at: Option<Instant>,
}

impl InflightMessage {
    pub fn reinit_instants(&mut self) {
        self.message.reinit_instants();
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let now_instant = Instant::now();
        self.visible_at = Some(if self.visible_at_secs > now_epoch {
            now_instant + Duration::from_secs(self.visible_at_secs - now_epoch)
        } else {
            now_instant
        });
    }
}

/// Per-account/region SQS state.
#[derive(Debug, Default)]
pub struct SqsState {
    /// Queue name → Queue (DashMap for concurrent access)
    pub queues: DashMap<String, Queue>,
}

/// Serializable snapshot of `SqsState`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SqsStateSnapshot {
    pub queues: Vec<QueueSnapshot>,
}

/// Serializable snapshot of a single queue.
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub name: String,
    pub url: String,
    pub arn: String,
    pub attributes: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    pub messages: VecDeque<Message>,
    /// Inflight messages are stored so they can be re-queued on restore.
    pub inflight: Vec<InflightMessage>,
    pub is_fifo: bool,
    pub created_at: String,
    /// FIFO dedup cache: dedup_id → (expiry epoch secs, message_id)
    pub dedup_cache: HashMap<String, (u64, String)>,
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
            .filter(|m| m.visible_at.map_or(true, |v| v <= now))
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
