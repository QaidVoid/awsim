//! Bulk-seed SQS queues + messages. SqsState is fully public so we
//! drive it directly from the AccountRegionStore.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AccountRegionStore, Body};
use awsim_sqs::state::{Message, Queue, SqsState};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use super::{fake_sentence, fake_slug};

#[derive(Deserialize)]
pub struct SeedSqsBody {
    pub queues: u64,
    #[serde(default)]
    pub messages_per_queue: u64,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Clone)]
pub struct SeedSqsState {
    pub store: AccountRegionStore<SqsState>,
    pub default_account: String,
    pub default_region: String,
    pub default_port: u16,
}

const MAX_QUEUES: u64 = 1_000;
const MAX_MESSAGES_PER_QUEUE: u64 = 100_000;

pub async fn seed(
    State(state): State<Arc<SeedSqsState>>,
    Json(body): Json<SeedSqsBody>,
) -> Response {
    if body.queues == 0 {
        return Json(json!({ "queues_created": 0, "messages_created": 0 })).into_response();
    }
    if body.queues > MAX_QUEUES {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("queues must be ≤ {MAX_QUEUES}"),
            })),
        )
            .into_response();
    }
    if body.messages_per_queue > MAX_MESSAGES_PER_QUEUE {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ValidationException",
                "message": format!("messages_per_queue must be ≤ {MAX_MESSAGES_PER_QUEUE}"),
            })),
        )
            .into_response();
    }

    let account = body
        .account
        .unwrap_or_else(|| state.default_account.clone());
    let region = body.region.unwrap_or_else(|| state.default_region.clone());
    let prefix = body.prefix.unwrap_or_else(|| "seed".to_string());
    let port = state.default_port;

    let result = tokio::task::spawn_blocking(move || {
        let sqs_state = state.store.get(&account, &region);
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now_iso = chrono_now_rfc3339();

        let mut queues_created = 0u64;
        let mut messages_created = 0u64;

        for _ in 0..body.queues {
            let name = format!("{prefix}-{}", fake_slug(2));
            if sqs_state.queues.contains_key(&name) {
                continue;
            }
            let url = format!("http://localhost:{port}/{account}/{name}");
            let arn = format!("arn:aws:sqs:{region}:{account}:{name}");
            let mut queue = Queue::new(
                name.clone(),
                url,
                arn,
                false,
                now_iso.clone(),
                HashMap::new(),
            );

            let mut messages = VecDeque::with_capacity(body.messages_per_queue as usize);
            for _ in 0..body.messages_per_queue {
                let body_text = fake_sentence();
                let md5 = md5_hex(body_text.as_bytes());
                messages.push_back(Message {
                    message_id: Uuid::new_v4().to_string(),
                    body: Body::InMemory(body_text.into_bytes()),
                    md5_of_body: md5,
                    attributes: HashMap::new(),
                    message_attributes: HashMap::new(),
                    sent_at_secs: now_secs,
                    delay_until_secs: None,
                    sequence_number: None,
                    receive_count: 0,
                    dedup_id: None,
                    group_id: None,
                    sent_at: None,
                    delay_until: None,
                });
                messages_created += 1;
            }
            queue.messages = messages;
            sqs_state.queues.insert(name, queue);
            queues_created += 1;
        }

        (queues_created, messages_created)
    })
    .await;

    match result {
        Ok((q, m)) => {
            info!(target = "seed", queues = q, messages = m, "Seeded SQS");
            Json(json!({
                "queues_created": q,
                "messages_created": m,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "JoinError", "message": e.to_string() })),
        )
            .into_response(),
    }
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(32);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn chrono_now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
