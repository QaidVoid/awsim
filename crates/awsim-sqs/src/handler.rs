use std::path::Path;
use std::sync::Arc;

use awsim_core::{
    AccountRegionStore, AwsError, BlobInventory, Body, BodyStore, Protocol, RequestContext,
    ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use crate::operations::{
    attributes, change_visibility, create_queue, dead_letter, delete_message, delete_queue,
    get_queue_url, list_queues, message_move, permissions, purge_queue, receive_message,
    send_message, tags,
};
use crate::state::{SqsState, SqsStateSnapshot};

/// The SQS service handler.
pub struct SqsService {
    store: AccountRegionStore<SqsState>,
    body_store: Option<Arc<BodyStore>>,
}

impl SqsService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: None,
        }
    }

    pub fn with_data_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: Some(Arc::new(BodyStore::new(dir.as_ref().to_path_buf()))),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<SqsState> {
        let state = self.store.get(&ctx.account_id, &ctx.region);
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
    }

    pub fn store(&self) -> AccountRegionStore<SqsState> {
        self.store.clone()
    }

    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.as_ref()
    }

    pub const GROUPS: &'static [&'static str] = &["sqs"];

    fn rebind_bodies(&self) {
        let Some(bs) = &self.body_store else {
            return;
        };
        for (_, state) in self.store.iter_all() {
            state.set_body_store(Arc::clone(bs));
            for mut queue_entry in state.queues.iter_mut() {
                let queue_name = queue_entry.key().clone();
                let queue = queue_entry.value_mut();
                for msg in queue.messages.iter_mut() {
                    if let Ok(path) = bs.blob_path("sqs", &queue_name, &msg.message_id) {
                        msg.body = Body::OnDisk(path);
                    }
                }
                for im in queue.inflight.values_mut() {
                    if let Ok(path) = bs.blob_path("sqs", &queue_name, &im.message.message_id) {
                        im.message.body = Body::OnDisk(path);
                    }
                }
            }
        }
    }
}

impl Default for SqsService {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobInventory for SqsService {
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (_, state) in self.store.iter_all() {
            for queue_entry in state.queues.iter() {
                let queue_name = queue_entry.key().clone();
                let queue = queue_entry.value();
                for msg in queue.messages.iter() {
                    if matches!(msg.body, Body::OnDisk(_)) {
                        out.push((
                            "sqs".to_string(),
                            queue_name.clone(),
                            msg.message_id.clone(),
                        ));
                    }
                }
                for im in queue.inflight.values() {
                    if matches!(im.message.body, Body::OnDisk(_)) {
                        out.push((
                            "sqs".to_string(),
                            queue_name.clone(),
                            im.message.message_id.clone(),
                        ));
                    }
                }
            }
        }
        out
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

        let state = self.get_state(ctx);

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
            "ChangeMessageVisibilityBatch" => change_visibility::handle_batch(&state, &input, ctx),
            "ListDeadLetterSourceQueues" => {
                dead_letter::list_dead_letter_source_queues(&state, &input, ctx)
            }
            "PurgeQueue" => purge_queue::handle(&state, &input, ctx),
            "TagQueue" => tags::tag_queue(&state, &input, ctx),
            "UntagQueue" => tags::untag_queue(&state, &input, ctx),
            "ListQueueTags" => tags::list_queue_tags(&state, &input, ctx),
            "AddPermission" => permissions::add_permission(&state, &input, ctx),
            "RemovePermission" => permissions::remove_permission(&state, &input, ctx),
            "StartMessageMoveTask" => message_move::start_message_move_task(&state, &input, ctx),
            "CancelMessageMoveTask" => message_move::cancel_message_move_task(&state, &input, ctx),
            "ListMessageMoveTasks" => message_move::list_message_move_tasks(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "CreateQueue"
            | "DeleteQueue"
            | "ListQueues"
            | "GetQueueUrl"
            | "GetQueueAttributes"
            | "SetQueueAttributes"
            | "SendMessage"
            | "SendMessageBatch"
            | "ReceiveMessage"
            | "DeleteMessage"
            | "DeleteMessageBatch"
            | "ChangeMessageVisibility"
            | "ChangeMessageVisibilityBatch"
            | "ListDeadLetterSourceQueues"
            | "PurgeQueue"
            | "TagQueue"
            | "UntagQueue"
            | "ListQueueTags"
            | "AddPermission"
            | "RemovePermission"
            | "StartMessageMoveTask"
            | "CancelMessageMoveTask"
            | "ListMessageMoveTasks" => Some(format!("sqs:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(&self, operation: &str, input: &Value, ctx: &RequestContext) -> Option<String> {
        match operation {
            "ListQueues" | "CreateQueue" | "GetQueueUrl" | "ListMessageMoveTasks" => {
                Some("*".to_string())
            }
            _ => {
                let queue_url = input.get("QueueUrl").and_then(|v| v.as_str())?;
                let name = queue_url.rsplit('/').next().filter(|s| !s.is_empty())?;
                Some(format!(
                    "arn:aws:sqs:{}:{}:{}",
                    ctx.region, ctx.account_id, name
                ))
            }
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.store.snapshot_to_bytes()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        use crate::state::SqsRegionSnapshot;
        use awsim_core::Snapshottable;

        if let Ok(()) = self.store.restore_from_bytes(data) {
            self.rebind_bodies();
            return Ok(());
        }

        let legacy: SqsStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let mut by_region: std::collections::HashMap<(String, String), Vec<_>> =
            std::collections::HashMap::new();
        for qs in legacy.queues {
            let parts: Vec<&str> = qs.arn.splitn(6, ':').collect();
            let key = if parts.len() == 6 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };
            by_region.entry(key).or_default().push(qs);
        }
        self.store.clear();
        for ((account_id, region), queues) in by_region {
            let snap = SqsRegionSnapshot {
                account_id: account_id.clone(),
                region: region.clone(),
                queues,
            };
            let (acct, reg, state) = SqsState::from_snapshot(snap);
            self.store.set(&acct, &reg, state);
        }
        self.rebind_bodies();
        Ok(())
    }
}
