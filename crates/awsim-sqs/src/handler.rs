use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    attributes, change_visibility, create_queue, delete_message, delete_queue, get_queue_url,
    list_queues, purge_queue, receive_message, send_message, tags,
};
use crate::state::{InflightMessage, Queue, QueueSnapshot, SqsState, SqsStateSnapshot, parse_redrive_policy_from_attrs};

/// The SQS service handler.
pub struct SqsService {
    store: AccountRegionStore<SqsState>,
}

impl SqsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for SqsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for SqsService {
    fn service_name(&self) -> &str {
        "sqs"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_0
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "SQS operation");

        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateQueue" => create_queue::handle(&state, &input, ctx),
            "DeleteQueue" => delete_queue::handle(&state, &input, ctx),
            "ListQueues" => list_queues::handle(&state, &input, ctx),
            "GetQueueUrl" => get_queue_url::handle(&state, &input, ctx),
            "GetQueueAttributes" => attributes::get_queue_attributes(&state, &input, ctx),
            "SetQueueAttributes" => attributes::set_queue_attributes(&state, &input, ctx),
            "SendMessage" => send_message::handle(&state, &input, ctx),
            "SendMessageBatch" => send_message::handle_batch(&state, &input, ctx),
            "ReceiveMessage" => receive_message::handle(&state, &input, ctx),
            "DeleteMessage" => delete_message::handle(&state, &input, ctx),
            "DeleteMessageBatch" => delete_message::handle_batch(&state, &input, ctx),
            "ChangeMessageVisibility" => change_visibility::handle(&state, &input, ctx),
            "PurgeQueue" => purge_queue::handle(&state, &input, ctx),
            "TagQueue" => tags::tag_queue(&state, &input, ctx),
            "UntagQueue" => tags::untag_queue(&state, &input, ctx),
            "ListQueueTags" => tags::list_queue_tags(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut queue_snapshots: Vec<QueueSnapshot> = Vec::new();

        for ((_account_id, _region), state) in self.store.iter_all() {
            for queue_entry in state.queues.iter() {
                let q = queue_entry.value();

                // Convert dedup_cache: Instant → epoch secs
                let dedup_cache: HashMap<String, (u64, String)> = q
                    .dedup_cache
                    .iter()
                    .map(|(k, (expiry, msg_id))| {
                        // Approximate expiry as epoch seconds
                        let secs_remaining = expiry
                            .checked_duration_since(Instant::now())
                            .unwrap_or(Duration::ZERO)
                            .as_secs();
                        (k.clone(), (now_epoch + secs_remaining, msg_id.clone()))
                    })
                    .collect();

                let inflight: Vec<InflightMessage> = q
                    .inflight
                    .values()
                    .cloned()
                    .collect();

                queue_snapshots.push(QueueSnapshot {
                    name: q.name.clone(),
                    url: q.url.clone(),
                    arn: q.arn.clone(),
                    attributes: q.attributes.clone(),
                    tags: q.tags.clone(),
                    messages: q.messages.clone(),
                    inflight,
                    is_fifo: q.is_fifo,
                    created_at: q.created_at.clone(),
                    dedup_cache,
                    redrive_policy: q.redrive_policy.clone(),
                });
            }
        }

        let snapshot = SqsStateSnapshot { queues: queue_snapshots };
        serde_json::to_vec(&snapshot).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: SqsStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let now_instant = Instant::now();

        for mut qs in snapshot.queues {
            // Derive account and region from the queue ARN.
            // ARN format: arn:aws:sqs:{region}:{account}:{name}
            let parts: Vec<&str> = qs.arn.splitn(6, ':').collect();
            let (account_id, region) = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };

            let state = self.store.get(&account_id, &region);

            // Convert dedup_cache: epoch secs → Instant
            let dedup_cache: HashMap<String, (Instant, String)> = qs
                .dedup_cache
                .iter()
                .filter_map(|(k, (expiry_secs, msg_id))| {
                    if *expiry_secs > now_epoch {
                        let remaining = Duration::from_secs(expiry_secs - now_epoch);
                        Some((k.clone(), (now_instant + remaining, msg_id.clone())))
                    } else {
                        None // expired; skip
                    }
                })
                .collect();

            // Reinit instants on messages
            for msg in qs.messages.iter_mut() {
                msg.reinit_instants();
            }

            // Inflight messages: restore those whose visibility timeout hasn't expired yet;
            // otherwise re-enqueue them so they're immediately receivable.
            let mut inflight: HashMap<String, InflightMessage> = HashMap::new();
            for mut im in qs.inflight {
                im.reinit_instants();
                if im.visible_at_secs > now_epoch {
                    inflight.insert(im.receipt_handle.clone(), im);
                } else {
                    // Visibility expired — return to queue
                    let mut msg = im.message;
                    msg.receive_count += 1;
                    qs.messages.push_front(msg);
                }
            }

            // Re-derive redrive_policy from attributes (covers old snapshots without the field)
            let redrive_policy = qs.redrive_policy
                .or_else(|| parse_redrive_policy_from_attrs(&qs.attributes));

            let queue = Queue {
                name: qs.name.clone(),
                url: qs.url.clone(),
                arn: qs.arn.clone(),
                attributes: qs.attributes,
                tags: qs.tags,
                messages: qs.messages,
                inflight,
                is_fifo: qs.is_fifo,
                created_at: qs.created_at,
                dedup_cache,
                redrive_policy,
            };

            state.queues.insert(qs.name, queue);
        }

        Ok(())
    }
}
