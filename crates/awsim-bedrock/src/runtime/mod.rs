//! Bedrock runtime translator dispatch.
//!
//! When a `BedrockBackend` is configured, each Bedrock-flavoured
//! request is routed to the per-vendor translator that converts it
//! to OpenAI-compatible chat.completions / embeddings calls and
//! shapes the response back into Bedrock's native format.
//!
//! When no backend is configured (or the backend is unreachable),
//! we fall back to deterministic canned responses so SDK code that
//! just wires up the calls keeps working in CI.

use std::sync::Arc;

use arc_swap::ArcSwap;
use awsim_core::error::ErrorType;
use awsim_core::{AwsError, HandlerByteStream, HandlerResult};
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use serde_json::Value;
use tracing::debug;

use crate::backend::{BedrockBackends, ResolvedTarget};
use crate::metrics::{AttemptRecord, InvocationRecord, OpKind, Outcome};
use chrono::Utc;
use std::time::Instant;

/// Translate an upstream HTTP status + body into a
/// Bedrock-shape `AwsError` so SDK retry / error-handling logic
/// matches what real Bedrock would produce.
///
/// Mapping mirrors the AWS Bedrock error catalogue:
/// - 400          -> `ValidationException`
/// - 401 / 403    -> `AccessDeniedException`
/// - 404          -> `ResourceNotFoundException`
/// - 408          -> `ModelTimeoutException`
/// - 413          -> `ValidationException` (oversized request)
/// - 429          -> `ThrottlingException` (this is the one that
///   actually drives SDK exponential-backoff retries)
/// - 5xx / other  -> `InternalServerException`
///
/// The upstream body is appended verbatim so consumers see the
/// underlying provider message (e.g. Groq's
/// "Rate limit reached for model ..., try again in 7.335s").
fn map_upstream_error(status: reqwest::StatusCode, body: &str) -> AwsError {
    use reqwest::StatusCode;
    let summary = body.trim();
    let summary = if summary.len() > 1024 {
        &summary[..1024]
    } else {
        summary
    };
    let (mapped_status, code, error_type) = match status.as_u16() {
        400 => (
            StatusCode::BAD_REQUEST,
            "ValidationException",
            ErrorType::Sender,
        ),
        401 | 403 => (
            StatusCode::FORBIDDEN,
            "AccessDeniedException",
            ErrorType::Sender,
        ),
        404 => (
            StatusCode::NOT_FOUND,
            "ResourceNotFoundException",
            ErrorType::Sender,
        ),
        408 => (
            StatusCode::REQUEST_TIMEOUT,
            "ModelTimeoutException",
            ErrorType::Receiver,
        ),
        413 => (
            StatusCode::BAD_REQUEST,
            "ValidationException",
            ErrorType::Sender,
        ),
        429 => (
            StatusCode::TOO_MANY_REQUESTS,
            "ThrottlingException",
            ErrorType::Sender,
        ),
        500..=599 => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalServerException",
            ErrorType::Receiver,
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalServerException",
            ErrorType::Receiver,
        ),
    };
    AwsError {
        status: mapped_status,
        code: code.to_string(),
        message: format!("Bedrock backend returned {summary}"),
        error_type,
        extras: None,
    }
}

mod anthropic;
mod canned;
mod cohere;
mod cohere_embed;
mod converse;
mod llama;
mod mistral;
mod openai;
mod titan;
mod titan_embed;

/// Shared backend caller for the per-vendor translators.
/// Builds the OpenAI ChatRequest via `build` (so each translator
/// owns the per-vendor field name shapes) and POSTs to
/// `<endpoint>/chat/completions`. Returns the raw OpenAI response;
/// translators shape it back into their own envelope.
async fn call_chat(
    backends: &BedrockBackends,
    bedrock_id: &str,
    build: impl Fn(&str) -> Result<openai::ChatRequest, AwsError>,
) -> Result<openai::ChatResponse, AwsError> {
    let candidates = backends.resolve_invoke_all(bedrock_id);
    if candidates.is_empty() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No backend mapping for Bedrock model {bedrock_id}"),
        ));
    }
    let started = Instant::now();
    let mut attempts: Vec<AttemptRecord> = Vec::with_capacity(candidates.len());
    let mut last_err: Option<AwsError> = None;
    let total = candidates.len();
    for (i, target) in candidates.iter().enumerate() {
        let attempt_start = Instant::now();
        let attempt = try_chat_once(target, &build).await;
        let latency_ms = attempt_start.elapsed().as_millis() as u64;
        let (outcome, err) = classify_for_metrics(&attempt);
        attempts.push(AttemptRecord {
            backend: target.name().to_string(),
            tag: target.tag.to_string(),
            outcome,
            latency_ms,
            error: err,
        });
        match attempt {
            Attempt::Ok(resp) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::Chat,
                    attempts,
                    Outcome::Success,
                    started.elapsed().as_millis() as u64,
                );
                return Ok(resp);
            }
            Attempt::Retriable(e) => {
                if i + 1 < total {
                    debug!(
                        backend = target.name(),
                        attempt = i + 1,
                        of = total,
                        "chat call hit retriable error; trying next alias target"
                    );
                }
                last_err = Some(e);
            }
            Attempt::Fatal(e) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::Chat,
                    attempts,
                    Outcome::FatalError,
                    started.elapsed().as_millis() as u64,
                );
                return Err(e);
            }
        }
    }
    record_invocation(
        backends,
        bedrock_id,
        OpKind::Chat,
        attempts,
        Outcome::RetriableError,
        started.elapsed().as_millis() as u64,
    );
    Err(last_err.unwrap_or_else(|| AwsError::internal("Bedrock backend chat: no attempts ran")))
}

/// Per-candidate attempt outcome. `Retriable` lets the caller roll
/// forward to the next alias target; `Fatal` aborts immediately
/// because retrying would not change the answer (e.g. a 400
/// because the request is malformed, or a translator-side build
/// error).
enum Attempt<T> {
    Ok(T),
    Retriable(AwsError),
    Fatal(AwsError),
}

/// Classify an attempt into the metrics outcome enum + a trimmed
/// error message (or `None` on success).
fn classify_for_metrics<T>(attempt: &Attempt<T>) -> (Outcome, Option<String>) {
    match attempt {
        Attempt::Ok(_) => (Outcome::Success, None),
        Attempt::Retriable(e) => (Outcome::RetriableError, Some(e.message.clone())),
        Attempt::Fatal(e) => (Outcome::FatalError, Some(e.message.clone())),
    }
}

/// Push one outer-call record to the ring + bump the per-mapping
/// counters. No-ops cleanly when the registries are absent (CLI
/// single-backend or tests).
fn record_invocation(
    backends: &BedrockBackends,
    bedrock_id: &str,
    op: OpKind,
    attempts: Vec<AttemptRecord>,
    outcome: Outcome,
    total_latency_ms: u64,
) {
    if let Some(m) = backends.metrics() {
        for a in &attempts {
            m.record(
                bedrock_id,
                &a.backend,
                a.outcome,
                a.latency_ms,
                a.error.as_deref(),
            );
        }
    }
    if let Some(r) = backends.recent() {
        r.push(InvocationRecord {
            at: Utc::now(),
            bedrock_id: bedrock_id.to_string(),
            op,
            attempts,
            outcome,
            total_latency_ms,
        });
    }
}

async fn try_chat_once(
    target: &ResolvedTarget<'_>,
    build: &impl Fn(&str) -> Result<openai::ChatRequest, AwsError>,
) -> Attempt<openai::ChatResponse> {
    let mut req = match build(target.tag) {
        Ok(r) => r,
        Err(e) => return Attempt::Fatal(e),
    };
    apply_chat_overrides(&mut req, target);
    let backend = target.backend;
    let url = format!("{}/chat/completions", backend.endpoint());
    let (status, body_text) = match post_chat(backend, &url, &req, target.timeout()).await {
        Ok(pair) => pair,
        Err(e) => return Attempt::Retriable(e),
    };
    if status.is_success() {
        return match serde_json::from_str::<openai::ChatResponse>(&body_text) {
            Ok(v) => Attempt::Ok(v),
            Err(e) => Attempt::Fatal(AwsError::internal(format!(
                "Bedrock backend JSON parse failed: {e}"
            ))),
        };
    }
    if status == reqwest::StatusCode::BAD_REQUEST
        && needs_string_content_fallback(&body_text)
        && flatten_request_content(&mut req)
    {
        debug!("Bedrock backend rejected multimodal content array, retrying with flattened text");
        match post_chat(backend, &url, &req, target.timeout()).await {
            Ok((status2, body2)) if status2.is_success() => {
                return match serde_json::from_str::<openai::ChatResponse>(&body2) {
                    Ok(v) => Attempt::Ok(v),
                    Err(e) => Attempt::Fatal(AwsError::internal(format!(
                        "Bedrock backend JSON parse failed: {e}"
                    ))),
                };
            }
            Ok((status2, body2)) => return classify_response(status2, &body2),
            Err(e) => return Attempt::Retriable(e),
        }
    }
    classify_response(status, &body_text)
}

/// Pin the request's max_tokens / temperature to whatever overrides
/// the alias target declared, leaving the translator-built default
/// in place when an override is absent.
fn apply_chat_overrides(req: &mut openai::ChatRequest, target: &ResolvedTarget<'_>) {
    if let Some(mt) = target.max_tokens {
        req.max_tokens = Some(mt);
    }
    if let Some(t) = target.temperature {
        req.temperature = Some(t);
    }
}

/// Map an upstream non-2xx response to an `Attempt`, deciding
/// retriability from the raw HTTP status before translation: 408,
/// 429, and 5xx are retriable; everything else (4xx other than
/// those two) is fatal because retrying would just route the same
/// bad request to the next backend.
fn classify_response<T>(status: reqwest::StatusCode, body: &str) -> Attempt<T> {
    let mapped = map_upstream_error(status, body);
    if is_retriable_status(status) {
        Attempt::Retriable(mapped)
    } else {
        Attempt::Fatal(mapped)
    }
}

fn is_retriable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500..=599)
}

/// POST the OpenAI chat request and return the raw `(status, body)`
/// pair. Keeping it factored out lets the call sites retry with a
/// modified request without rebuilding the http client setup.
async fn post_chat(
    backend: &crate::backend::BedrockBackend,
    url: &str,
    req: &openai::ChatRequest,
    timeout_override: Option<std::time::Duration>,
) -> Result<(reqwest::StatusCode, String), AwsError> {
    let mut http_req = backend.client().post(url).json(req);
    if let Some(key) = backend.api_key() {
        http_req = http_req.bearer_auth(key);
    }
    if let Some(t) = timeout_override {
        http_req = http_req.timeout(t);
    }
    let resp = http_req
        .send()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend POST {url} failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend body read failed: {e}")))?;
    Ok((status, body))
}

/// Detect the upstream 400 that backends raise when their OpenAI
/// compatibility surface only accepts a string `content` field. Groq,
/// gpt-oss-20b, and other text-only OpenAI-compat endpoints emit
/// `"content must be a string"` for multimodal requests; we use that
/// signal to flatten and retry once.
fn needs_string_content_fallback(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("content must be a string")
        || lower.contains("content: must be string")
        || lower.contains("content should be a string")
}

/// Replace every message's multimodal parts array with a single
/// joined text string. Image parts become an inline marker so the
/// model still knows an attachment was present; text parts join with
/// newlines. Returns true if any message actually changed shape.
fn flatten_request_content(req: &mut openai::ChatRequest) -> bool {
    let mut changed = false;
    for m in &mut req.messages {
        if let openai::MessageContent::Parts(parts) = &m.content {
            let mut text = String::new();
            for p in parts {
                if !text.is_empty() {
                    text.push('\n');
                }
                match p {
                    openai::ContentPart::Text { text: t } => text.push_str(t),
                    openai::ContentPart::ImageUrl { image_url } => {
                        if image_url.url.starts_with("data:") {
                            text.push_str(
                                "[Image attachment omitted, backend does not accept inline images]",
                            );
                        } else {
                            text.push_str(&format!("[Image: {}]", image_url.url));
                        }
                    }
                }
            }
            m.content = openai::MessageContent::Text(text);
            changed = true;
        }
    }
    changed
}

/// Streaming variant: hits `/chat/completions` with `stream:true`,
/// drains the SSE response, and accumulates a flat
/// `(text, finish_reason, prompt_tokens, completion_tokens)` tuple.
/// Per-family streaming translators wrap the result in their native
/// chunk envelope. Wire-level vnd.amazon.eventstream framing is
/// future work — chunks are returned as a JSON array on the response.
pub(crate) async fn call_chat_stream(
    backends: &BedrockBackends,
    bedrock_id: &str,
    build: impl Fn(&str) -> Result<openai::ChatRequest, AwsError>,
) -> Result<AccumulatedStream, AwsError> {
    let candidates = backends.resolve_invoke_all(bedrock_id);
    if candidates.is_empty() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No backend mapping for Bedrock model {bedrock_id}"),
        ));
    }
    // The current SSE path buffers the entire upstream response
    // before returning (see `accumulate_sse`), so per-candidate
    // retry is safe: no downstream bytes have left awsim yet when
    // we discover the upstream failed. Once Phase 6 / real
    // eventstream framing pipes bytes through incrementally, retry
    // becomes safe only up to the first forwarded byte.
    let started = Instant::now();
    let mut attempts: Vec<AttemptRecord> = Vec::with_capacity(candidates.len());
    let mut last_err: Option<AwsError> = None;
    let total = candidates.len();
    for (i, target) in candidates.iter().enumerate() {
        let attempt_start = Instant::now();
        let attempt = try_chat_stream_once(target, &build).await;
        let latency_ms = attempt_start.elapsed().as_millis() as u64;
        let (outcome, err) = classify_for_metrics(&attempt);
        attempts.push(AttemptRecord {
            backend: target.name().to_string(),
            tag: target.tag.to_string(),
            outcome,
            latency_ms,
            error: err,
        });
        match attempt {
            Attempt::Ok(acc) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::ChatStream,
                    attempts,
                    Outcome::Success,
                    started.elapsed().as_millis() as u64,
                );
                return Ok(acc);
            }
            Attempt::Retriable(e) => {
                if i + 1 < total {
                    debug!(
                        backend = target.name(),
                        attempt = i + 1,
                        of = total,
                        "chat stream hit retriable error; trying next alias target"
                    );
                }
                last_err = Some(e);
            }
            Attempt::Fatal(e) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::ChatStream,
                    attempts,
                    Outcome::FatalError,
                    started.elapsed().as_millis() as u64,
                );
                return Err(e);
            }
        }
    }
    record_invocation(
        backends,
        bedrock_id,
        OpKind::ChatStream,
        attempts,
        Outcome::RetriableError,
        started.elapsed().as_millis() as u64,
    );
    Err(last_err
        .unwrap_or_else(|| AwsError::internal("Bedrock backend chat stream: no attempts ran")))
}

async fn try_chat_stream_once(
    target: &ResolvedTarget<'_>,
    build: &impl Fn(&str) -> Result<openai::ChatRequest, AwsError>,
) -> Attempt<AccumulatedStream> {
    let mut req = match build(target.tag) {
        Ok(r) => r,
        Err(e) => return Attempt::Fatal(e),
    };
    apply_chat_overrides(&mut req, target);
    req.stream = Some(true);
    req.stream_options = Some(openai::StreamOptions {
        include_usage: true,
    });

    let backend = target.backend;
    let url = format!("{}/chat/completions", backend.endpoint());
    let (status, raw) = match post_chat(backend, &url, &req, target.timeout()).await {
        Ok(pair) => pair,
        Err(e) => return Attempt::Retriable(e),
    };
    if status.is_success() {
        return Attempt::Ok(accumulate_sse(&raw));
    }
    if status == reqwest::StatusCode::BAD_REQUEST
        && needs_string_content_fallback(&raw)
        && flatten_request_content(&mut req)
    {
        debug!(
            "Bedrock backend rejected multimodal content array, retrying stream with flattened text"
        );
        match post_chat(backend, &url, &req, target.timeout()).await {
            Ok((status2, raw2)) if status2.is_success() => {
                return Attempt::Ok(accumulate_sse(&raw2));
            }
            Ok((status2, raw2)) => return classify_response(status2, &raw2),
            Err(e) => return Attempt::Retriable(e),
        }
    }
    classify_response(status, &raw)
}

#[derive(Debug, Default)]
pub(crate) struct AccumulatedStream {
    pub text: String,
    pub finish_reason: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub tool_calls: Vec<AccumulatedToolCall>,
}

/// Per-index tool-call buffer. OpenAI streams tool calls as
/// fragments: a chunk may carry just the `id`, just a slice of the
/// function name, or just a slice of the JSON `arguments` string.
/// We append everything by index so the assembled record is ready
/// to forward back to the caller once the stream closes.
#[derive(Debug, Default, Clone)]
pub(crate) struct AccumulatedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub(crate) fn accumulate_sse(raw: &str) -> AccumulatedStream {
    let mut acc = AccumulatedStream::default();
    for line in raw.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let chunk: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
            acc.text.push_str(delta);
        }
        if let Some(tcs) = chunk["choices"][0]["delta"]["tool_calls"].as_array() {
            for tc in tcs {
                accumulate_tool_call_delta(&mut acc.tool_calls, tc);
            }
        }
        if let Some(fr) = chunk["choices"][0]["finish_reason"].as_str() {
            acc.finish_reason = Some(fr.to_string());
        }
        if let Some(p) = chunk["usage"]["prompt_tokens"].as_u64() {
            acc.prompt_tokens = p as u32;
        }
        if let Some(c) = chunk["usage"]["completion_tokens"].as_u64() {
            acc.completion_tokens = c as u32;
        }
    }
    acc
}

/// Append one tool-call delta into the per-index slot. Indices are
/// not guaranteed dense across chunks so we grow the buffer with
/// blank slots as needed; missing fields stay empty until a later
/// chunk supplies them.
pub(crate) fn accumulate_tool_call_delta(slots: &mut Vec<AccumulatedToolCall>, delta: &Value) {
    let index = delta["index"].as_u64().unwrap_or(0) as usize;
    while slots.len() <= index {
        slots.push(AccumulatedToolCall::default());
    }
    let slot = &mut slots[index];
    if let Some(id) = delta["id"].as_str() {
        slot.id.push_str(id);
    }
    if let Some(name) = delta["function"]["name"].as_str() {
        slot.name.push_str(name);
    }
    if let Some(args) = delta["function"]["arguments"].as_str() {
        slot.arguments.push_str(args);
    }
}

/// Wrap per-family streaming chunks for `InvokeModelWithResponseStream`.
/// AWS emits each vendor chunk under an event-stream message tagged
/// `:event-type=chunk`, with the chunk JSON base64-encoded under a
/// `bytes` field. The protocol layer recognises the
/// `__awsim_eventstream__` marker and turns these descriptors into
/// the right binary frame format on the wire.
pub(crate) fn stream_envelope(chunks: Vec<Value>) -> Value {
    use base64::Engine;
    let frames: Vec<Value> = chunks
        .into_iter()
        .map(|chunk| {
            let json = serde_json::to_vec(&chunk).unwrap_or_default();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&json);
            serde_json::json!({
                "headers": {
                    ":event-type": "chunk",
                    ":content-type": "application/json",
                    ":message-type": "event",
                },
                "payload": { "bytes": b64 }
            })
        })
        .collect();
    serde_json::json!({
        "__awsim_eventstream__": frames
    })
}

/// Wrap a list of typed Converse-stream events (each is `{ "<eventType>":
/// <payload> }`) into the protocol-layer event-stream marker shape.
/// Each event becomes its own binary frame whose `:event-type` header
/// names the variant — `messageStart`, `contentBlockDelta`,
/// `contentBlockStop`, `messageStop`, `metadata`.
pub(crate) fn converse_stream_envelope(events: Vec<Value>) -> Value {
    let frames: Vec<Value> = events
        .into_iter()
        .filter_map(|event| {
            // Each event is a single-key object whose key is the
            // event-type and whose value is the payload.
            let obj = event.as_object()?;
            let (event_type, payload) = obj.iter().next()?;
            Some(serde_json::json!({
                "headers": {
                    ":event-type": event_type,
                    ":content-type": "application/json",
                    ":message-type": "event",
                },
                "payload": payload
            }))
        })
        .collect();
    serde_json::json!({
        "__awsim_eventstream__": frames
    })
}

/// Same shape as `call_chat` but for `/v1/embeddings`. Resolves the
/// Bedrock id via `resolve_embed` so the embed-only mappings in the
/// model map take precedence.
async fn call_embed(
    backends: &BedrockBackends,
    bedrock_id: &str,
    build: impl Fn(&str) -> Result<openai::EmbeddingsRequest, AwsError>,
) -> Result<openai::EmbeddingsResponse, AwsError> {
    let candidates = backends.resolve_embed_all(bedrock_id);
    if candidates.is_empty() {
        return Err(AwsError::not_found(
            "ResourceNotFoundException",
            format!("No backend mapping for Bedrock embedding model {bedrock_id}"),
        ));
    }
    let started = Instant::now();
    let mut attempts: Vec<AttemptRecord> = Vec::with_capacity(candidates.len());
    let mut last_err: Option<AwsError> = None;
    let total = candidates.len();
    for (i, target) in candidates.iter().enumerate() {
        let attempt_start = Instant::now();
        let attempt = try_embed_once(target, &build).await;
        let latency_ms = attempt_start.elapsed().as_millis() as u64;
        let (outcome, err) = classify_for_metrics(&attempt);
        attempts.push(AttemptRecord {
            backend: target.name().to_string(),
            tag: target.tag.to_string(),
            outcome,
            latency_ms,
            error: err,
        });
        match attempt {
            Attempt::Ok(resp) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::Embed,
                    attempts,
                    Outcome::Success,
                    started.elapsed().as_millis() as u64,
                );
                return Ok(resp);
            }
            Attempt::Retriable(e) => {
                if i + 1 < total {
                    debug!(
                        backend = target.name(),
                        attempt = i + 1,
                        of = total,
                        "embed call hit retriable error; trying next alias target"
                    );
                }
                last_err = Some(e);
            }
            Attempt::Fatal(e) => {
                record_invocation(
                    backends,
                    bedrock_id,
                    OpKind::Embed,
                    attempts,
                    Outcome::FatalError,
                    started.elapsed().as_millis() as u64,
                );
                return Err(e);
            }
        }
    }
    record_invocation(
        backends,
        bedrock_id,
        OpKind::Embed,
        attempts,
        Outcome::RetriableError,
        started.elapsed().as_millis() as u64,
    );
    Err(last_err.unwrap_or_else(|| AwsError::internal("Bedrock backend embed: no attempts ran")))
}

async fn try_embed_once(
    target: &ResolvedTarget<'_>,
    build: &impl Fn(&str) -> Result<openai::EmbeddingsRequest, AwsError>,
) -> Attempt<openai::EmbeddingsResponse> {
    let req = match build(target.tag) {
        Ok(r) => r,
        Err(e) => return Attempt::Fatal(e),
    };
    // Embed targets honour timeout overrides only; the
    // OpenAI EmbeddingsRequest shape has no max_tokens /
    // temperature knobs, so the chat-only overrides are a
    // no-op here even when they're declared on the alias.
    let backend = target.backend;
    let url = format!("{}/embeddings", backend.endpoint());
    let mut http_req = backend.client().post(&url).json(&req);
    if let Some(key) = backend.api_key() {
        http_req = http_req.bearer_auth(key);
    }
    if let Some(t) = target.timeout() {
        http_req = http_req.timeout(t);
    }
    let resp = match http_req.send().await {
        Ok(r) => r,
        Err(e) => {
            return Attempt::Retriable(AwsError::internal(format!(
                "Bedrock backend POST {url} failed: {e}"
            )));
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return classify_response(status, &body_text);
    }
    match resp.json::<openai::EmbeddingsResponse>().await {
        Ok(v) => Attempt::Ok(v),
        Err(e) => Attempt::Fatal(AwsError::internal(format!(
            "Bedrock backend JSON parse failed: {e}"
        ))),
    }
}

/// Dispatch InvokeModel by Bedrock model-id prefix. Routes Anthropic
/// (`anthropic.claude-*`) to the proxy translator when a backend is
/// configured; everything else still hits the canned fallback (will
/// be expanded in subsequent commits).
pub async fn invoke_model(
    backends: Option<&BedrockBackends>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "InvokeModel");

    let body = extract_body(input)?;

    // When no backend is configured at all, the canned mock keeps SDK
    // wiring testable in CI / fresh-clone setups. As soon as the
    // operator points awsim at a real backend, we *propagate* errors
    // instead of masking them with a canned response - silently
    // returning a synthetic embedding when Ollama doesn't have the
    // embed model pulled (or the model id has no mapping in the
    // model_map) is the kind of failure that turns into a 4-hour
    // KB-indexing rabbit hole. Surface it instead.
    let Some(backends) = backends else {
        return canned::invoke_model(input);
    };

    match ModelFamily::for_id(model_id) {
        Some(ModelFamily::Anthropic) => anthropic::invoke(backends, model_id, &body).await,
        Some(ModelFamily::Titan) => titan::invoke(backends, model_id, &body).await,
        Some(ModelFamily::Llama) => llama::invoke(backends, model_id, &body).await,
        Some(ModelFamily::Mistral) => mistral::invoke(backends, model_id, &body).await,
        Some(ModelFamily::Cohere) => cohere::invoke(backends, model_id, &body).await,
        Some(ModelFamily::TitanEmbed) => titan_embed::invoke(backends, model_id, &body).await,
        Some(ModelFamily::CohereEmbed) => cohere_embed::invoke(backends, model_id, &body).await,
        Some(ModelFamily::Other) | None => Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "InvokeModel: no translator for `{model_id}`. Supported \
                 prefixes: anthropic.claude*, amazon.titan-text*, \
                 amazon.titan-embed*, meta.llama*, mistral.*, \
                 cohere.command*, cohere.embed*. Add a mapping under \
                 [invoke] / [embed] in your bedrock model map if you \
                 need a custom id."
            ),
        )),
    }
}

pub async fn invoke_model_with_response_stream(
    backends: Option<&BedrockBackends>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "InvokeModelWithResponseStream");
    let body = extract_body(input)?;

    let Some(backends) = backends else {
        return canned::invoke_model_with_response_stream(input);
    };

    match ModelFamily::for_id(model_id) {
        Some(ModelFamily::Anthropic) => {
            anthropic::invoke_streaming(backends, model_id, &body).await
        }
        Some(ModelFamily::Titan) => titan::invoke_streaming(backends, model_id, &body).await,
        Some(ModelFamily::Llama) => llama::invoke_streaming(backends, model_id, &body).await,
        Some(ModelFamily::Mistral) => mistral::invoke_streaming(backends, model_id, &body).await,
        Some(ModelFamily::Cohere) => cohere::invoke_streaming(backends, model_id, &body).await,
        Some(ModelFamily::TitanEmbed) | Some(ModelFamily::CohereEmbed) => {
            Err(AwsError::bad_request(
                "ValidationException",
                format!(
                    "InvokeModelWithResponseStream is not valid for embedding model `{model_id}`. \
                 Use InvokeModel instead."
                ),
            ))
        }
        Some(ModelFamily::Other) | None => Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "InvokeModelWithResponseStream: no translator for `{model_id}`. \
                 Supported prefixes: anthropic.claude*, amazon.titan-text*, \
                 meta.llama*, mistral.*, cohere.command*."
            ),
        )),
    }
}

pub async fn converse(
    backends: Option<&BedrockBackends>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "Converse");
    let Some(backends) = backends else {
        return canned::converse(input);
    };
    converse::invoke(backends, model_id, input).await
}

pub async fn converse_stream(
    backends: Option<&BedrockBackends>,
    input: &Value,
) -> Result<Value, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?;
    debug!(model_id = %model_id, "ConverseStream");
    let Some(backends) = backends else {
        return canned::converse_stream(input);
    };
    converse::invoke_streaming(backends, model_id, input).await
}

/// Pull the model body out of the parsed RestJson1 input.
///
/// The Bedrock REST contract is "send the model body as the raw
/// HTTP body" - `POST /model/{modelId}/invoke` with body
/// `{"inputText":"..."}`. The awsim REST parser deserializes that
/// JSON directly into the input object and merges path parameters
/// (just `modelId`) on top, so the body fields and the path param
/// arrive flattened together. Strip the path param and what's left
/// is the model body.
///
/// The `Some(body)` branches are retained for defensiveness - some
/// internal call sites (replay infra, hand-built test fixtures)
/// may wrap the body explicitly. Production SDK traffic never does.
fn extract_body(input: &Value) -> Result<Value, AwsError> {
    match input.get("body") {
        Some(Value::Object(_)) | Some(Value::Array(_)) => Ok(input["body"].clone()),
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| {
            AwsError::bad_request(
                "ValidationException",
                format!("body is not valid JSON: {e}"),
            )
        }),
        Some(Value::Null) | None => {
            let Value::Object(map) = input else {
                return Ok(Value::Object(serde_json::Map::new()));
            };
            // Path params merged in by `awsim-core`'s REST parser.
            // Bedrock's invoke / converse routes only declare
            // `modelId`, so dropping it leaves exactly the model
            // body the SDK sent on the wire.
            let mut body = map.clone();
            body.remove("modelId");
            Ok(Value::Object(body))
        }
        Some(other) => Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "body must be a JSON object or string, got {}",
                kind_of(other)
            ),
        )),
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[derive(Debug, Clone, Copy)]
enum ModelFamily {
    Anthropic,
    Titan,
    Llama,
    Mistral,
    Cohere,
    TitanEmbed,
    CohereEmbed,
    /// Catch-all for ids that aren't routed to a translator yet
    /// (image / unknown). Land in canned fallback.
    Other,
}

impl ModelFamily {
    fn for_id(id: &str) -> Option<Self> {
        if id.starts_with("anthropic.claude") {
            Some(Self::Anthropic)
        } else if id.starts_with("amazon.titan-text") {
            Some(Self::Titan)
        } else if id.starts_with("amazon.titan-embed") {
            Some(Self::TitanEmbed)
        } else if id.starts_with("meta.llama") {
            Some(Self::Llama)
        } else if id.starts_with("mistral.") {
            Some(Self::Mistral)
        } else if id.starts_with("cohere.command") {
            Some(Self::Cohere)
        } else if id.starts_with("cohere.embed") {
            Some(Self::CohereEmbed)
        } else {
            Some(Self::Other)
        }
    }
}

// ── Real streaming entry point ───────────────────────────────────────────────

/// Open a streaming response for `ConverseStream` /
/// `InvokeModelWithResponseStream`. Forwards each Ollama SSE chunk
/// to the client as its own AWS event-stream binary frame so the
/// caller sees tokens as they're produced, not after the full
/// response buffers.
///
/// Falls back to a single-frame canned stream when no backend is
/// configured or the resolved backend can't be reached — same
/// behaviour as the buffered path, just shipped as proper binary
/// frames.
pub(crate) async fn stream_response(
    backends: Arc<ArcSwap<Option<BedrockBackends>>>,
    operation: &str,
    input: Value,
) -> Result<HandlerResult, AwsError> {
    let model_id = input["modelId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "modelId is required"))?
        .to_string();
    let body = extract_body(&input)?;
    let is_converse = operation == "ConverseStream";

    let guard = backends.load();
    let registry = guard.as_ref().as_ref();

    let resolved = registry.and_then(|r| r.resolve_invoke(&model_id));
    let Some((backend, model_tag)) = resolved else {
        // No backend mapping — emit a single canned frame.
        return Ok(canned_stream(&model_id, is_converse));
    };

    if !is_converse {
        // Vendor-family chunked streaming is more involved — we'd
        // need per-family chunk translators that base64-wrap each
        // partial. Fall back to the buffered path (which already
        // emits proper binary frames) until we wire those up.
        let value = invoke_model_with_response_stream(registry, &input).await?;
        return Ok(buffered_stream_to_streaming(value));
    }

    // Build the OpenAI-compat chat request from the Converse input.
    let mut req = converse::to_openai_request(model_tag, &input)?;
    req.stream = Some(true);
    req.stream_options = Some(openai::StreamOptions {
        include_usage: true,
    });

    let url = format!("{}/chat/completions", backend.endpoint());
    let resp = open_chat_stream(backend, &url, &mut req).await?;

    // Header frame goes out before the SSE proxy starts so the
    // client sees the messageStart event right away even if the
    // model takes a few seconds to emit its first token.
    let header_frame =
        encode_event_frame("messageStart", &serde_json::json!({ "role": "assistant" }));

    let started = std::time::Instant::now();
    let translated = converse_stream_from_sse(resp.bytes_stream(), started);
    let header_stream =
        stream::once(async move { Ok::<Bytes, AwsError>(Bytes::from(header_frame)) });
    let combined: BoxStream<'static, Result<Bytes, AwsError>> =
        header_stream.chain(translated).boxed();
    void_use(body); // silence unused warning — body parsed for validation only
    Ok(HandlerResult::Streaming {
        body: combined,
        content_type: "application/vnd.amazon.eventstream",
    })
}

#[allow(clippy::needless_pass_by_value)]
fn void_use<T>(_: T) {}

/// Open a streaming response, retrying once with flattened content if
/// the backend rejects the multimodal parts array. Mirrors the
/// non-streaming fallback in `call_chat` so live ConverseStream calls
/// behave the same on text-only OpenAI-compat backends.
async fn open_chat_stream(
    backend: &crate::backend::BedrockBackend,
    url: &str,
    req: &mut openai::ChatRequest,
) -> Result<reqwest::Response, AwsError> {
    let resp = send_chat_stream(backend, url, req).await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend stream read failed: {e}")))?;
    if status == reqwest::StatusCode::BAD_REQUEST
        && needs_string_content_fallback(&body)
        && flatten_request_content(req)
    {
        debug!(
            "Bedrock backend rejected multimodal content array, retrying stream with flattened text"
        );
        let resp2 = send_chat_stream(backend, url, req).await?;
        if resp2.status().is_success() {
            return Ok(resp2);
        }
        let body2 = resp2
            .text()
            .await
            .map_err(|e| AwsError::internal(format!("Bedrock backend stream read failed: {e}")))?;
        return Err(map_upstream_error(status, &body2));
    }
    Err(map_upstream_error(status, &body))
}

async fn send_chat_stream(
    backend: &crate::backend::BedrockBackend,
    url: &str,
    req: &openai::ChatRequest,
) -> Result<reqwest::Response, AwsError> {
    let mut http_req = backend.client().post(url).json(req);
    if let Some(key) = backend.api_key() {
        http_req = http_req.bearer_auth(key);
    }
    http_req
        .send()
        .await
        .map_err(|e| AwsError::internal(format!("Bedrock backend POST {url} failed: {e}")))
}

/// State carried across each `unfold` step of the Converse SSE
/// translator. Public-in-private because `unfold`'s closure captures
/// it and recursive `state machine` patterns need a named type.
struct ConverseStreamState {
    inner: std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<Bytes>> + Send>>,
    buffer: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    finish_reason: Option<String>,
    started: std::time::Instant,
    done: bool,
    /// Per-index buffer for streamed tool calls. We assemble them as
    /// fragments arrive and flush the completed frames at EOF.
    tool_calls: Vec<AccumulatedToolCall>,
    /// After upstream EOF we still have to emit closing frames in
    /// sequence — this queue holds them.
    trailing: std::collections::VecDeque<Bytes>,
}

/// Translate Ollama's SSE chat-completion stream into AWS event-stream
/// binary frames (Converse format). Each token-bearing chunk becomes
/// a `contentBlockDelta` event; the final chunk emits the closing
/// `contentBlockStop` + `messageStop` + `metadata` frames.
fn converse_stream_from_sse(
    upstream: impl futures::Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    started: std::time::Instant,
) -> BoxStream<'static, Result<Bytes, AwsError>> {
    use futures::stream::unfold;

    let initial = ConverseStreamState {
        inner: Box::pin(upstream),
        buffer: String::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        finish_reason: None,
        started,
        done: false,
        tool_calls: Vec::new(),
        trailing: std::collections::VecDeque::new(),
    };

    unfold(initial, |mut st| async move {
        if st.done {
            return None;
        }
        if let Some(frame) = st.trailing.pop_front() {
            if st.trailing.is_empty() {
                st.done = true;
            }
            return Some((Ok(frame), st));
        }

        loop {
            if let Some(frame) = take_next_delta(&mut st) {
                return Some((Ok(frame), st));
            }
            match st.inner.next().await {
                Some(Ok(chunk)) => {
                    st.buffer.push_str(&String::from_utf8_lossy(&chunk));
                    continue;
                }
                Some(Err(e)) => {
                    st.done = true;
                    return Some((
                        Err(AwsError::internal(format!(
                            "Bedrock backend stream read failed: {e}"
                        ))),
                        st,
                    ));
                }
                None => {
                    let has_tools = !st.tool_calls.is_empty();
                    let stop_reason = if has_tools {
                        "tool_use"
                    } else {
                        converse::map_stop_reason(st.finish_reason.as_deref())
                    };
                    st.trailing.push_back(Bytes::from(encode_event_frame(
                        "contentBlockStop",
                        &serde_json::json!({ "contentBlockIndex": 0 }),
                    )));
                    for (i, tc) in st.tool_calls.iter().enumerate() {
                        let idx = i + 1;
                        st.trailing.push_back(Bytes::from(encode_event_frame(
                            "contentBlockStart",
                            &serde_json::json!({
                                "start": { "toolUse": { "toolUseId": tc.id, "name": tc.name } },
                                "contentBlockIndex": idx,
                            }),
                        )));
                        if !tc.arguments.is_empty() {
                            st.trailing.push_back(Bytes::from(encode_event_frame(
                                "contentBlockDelta",
                                &serde_json::json!({
                                    "delta": { "toolUse": { "input": tc.arguments } },
                                    "contentBlockIndex": idx,
                                }),
                            )));
                        }
                        st.trailing.push_back(Bytes::from(encode_event_frame(
                            "contentBlockStop",
                            &serde_json::json!({ "contentBlockIndex": idx }),
                        )));
                    }
                    st.trailing.push_back(Bytes::from(encode_event_frame(
                        "messageStop",
                        &serde_json::json!({ "stopReason": stop_reason }),
                    )));
                    st.trailing.push_back(Bytes::from(encode_event_frame(
                        "metadata",
                        &serde_json::json!({
                            "usage": {
                                "inputTokens":  st.prompt_tokens,
                                "outputTokens": st.completion_tokens,
                                "totalTokens":  st.prompt_tokens + st.completion_tokens,
                            },
                            "metrics": { "latencyMs": st.started.elapsed().as_millis() as u64 }
                        }),
                    )));
                    let frame = st.trailing.pop_front().expect("trailing seeded above");
                    if st.trailing.is_empty() {
                        st.done = true;
                    }
                    return Some((Ok(frame), st));
                }
            }
        }
    })
    .boxed()
}

/// Pull complete `data: …` lines out of the buffer one at a time.
/// Returns a `contentBlockDelta` frame when a chunk has text, or
/// `None` when the buffer doesn't yet hold a full event. Updates
/// usage/finish-reason counters as it sees them.
fn take_next_delta(st: &mut ConverseStreamState) -> Option<Bytes> {
    while let Some(newline_pos) = st.buffer.find('\n') {
        let line: String = st.buffer.drain(..=newline_pos).collect();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" || payload.is_empty() {
            continue;
        }
        let chunk: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(p) = chunk["usage"]["prompt_tokens"].as_u64() {
            st.prompt_tokens = p as u32;
        }
        if let Some(c) = chunk["usage"]["completion_tokens"].as_u64() {
            st.completion_tokens = c as u32;
        }
        if let Some(fr) = chunk["choices"][0]["finish_reason"].as_str() {
            st.finish_reason = Some(fr.to_string());
        }
        if let Some(tcs) = chunk["choices"][0]["delta"]["tool_calls"].as_array() {
            for tc in tcs {
                accumulate_tool_call_delta(&mut st.tool_calls, tc);
            }
        }
        if let Some(text) = chunk["choices"][0]["delta"]["content"].as_str()
            && !text.is_empty()
        {
            return Some(Bytes::from(encode_event_frame(
                "contentBlockDelta",
                &serde_json::json!({
                    "delta": { "text": text },
                    "contentBlockIndex": 0
                }),
            )));
        }
    }
    None
}

/// Encode a single Converse event into an AWS event-stream binary
/// frame (headers + JSON payload + CRC). Re-uses the encoder in
/// awsim-core::protocol::eventstream.
fn encode_event_frame(event_type: &str, payload: &Value) -> Vec<u8> {
    use awsim_core::protocol::eventstream::{EventHeader, append_message};
    let payload_bytes = serde_json::to_vec(payload).unwrap_or_default();
    let headers = vec![
        EventHeader {
            name: ":event-type".to_string(),
            value: event_type.to_string(),
        },
        EventHeader {
            name: ":content-type".to_string(),
            value: "application/json".to_string(),
        },
        EventHeader {
            name: ":message-type".to_string(),
            value: "event".to_string(),
        },
    ];
    let mut buf = Vec::with_capacity(64 + payload_bytes.len());
    append_message(&mut buf, &headers, &payload_bytes);
    buf
}

/// Single-frame canned stream — used when no backend is configured
/// or the model id has no mapping. Keeps the stream interface
/// consistent so the AI SDK's stream parser sees a valid (if short)
/// event sequence.
fn canned_stream(model_id: &str, is_converse: bool) -> HandlerResult {
    let canned_text = format!(
        "AWSim canned response for {model_id} — configure a Bedrock backend to proxy to a real LLM."
    );
    let frames: Vec<Vec<u8>> = if is_converse {
        vec![
            encode_event_frame("messageStart", &serde_json::json!({ "role": "assistant" })),
            encode_event_frame(
                "contentBlockDelta",
                &serde_json::json!({
                    "delta": { "text": &canned_text },
                    "contentBlockIndex": 0
                }),
            ),
            encode_event_frame(
                "contentBlockStop",
                &serde_json::json!({ "contentBlockIndex": 0 }),
            ),
            encode_event_frame(
                "messageStop",
                &serde_json::json!({ "stopReason": "end_turn" }),
            ),
            encode_event_frame(
                "metadata",
                &serde_json::json!({
                    "usage": { "inputTokens": 0, "outputTokens": 0, "totalTokens": 0 },
                    "metrics": { "latencyMs": 0 }
                }),
            ),
        ]
    } else {
        // InvokeModelWithResponseStream: single chunk event with the
        // canned text base64-wrapped under `bytes`.
        use base64::Engine;
        let payload = serde_json::json!({
            "completion": canned_text,
            "stop_reason": "end_turn",
        });
        let payload_b = serde_json::to_vec(&payload).unwrap_or_default();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&payload_b);
        vec![encode_chunk_frame(&b64)]
    };

    let body: HandlerByteStream = stream::iter(
        frames
            .into_iter()
            .map(|f| Ok::<Bytes, AwsError>(Bytes::from(f))),
    )
    .boxed();
    HandlerResult::Streaming {
        body,
        content_type: "application/vnd.amazon.eventstream",
    }
}

/// Single-chunk frame for `InvokeModelWithResponseStream` — wraps a
/// base64-encoded vendor JSON payload under `bytes`.
fn encode_chunk_frame(b64_payload: &str) -> Vec<u8> {
    use awsim_core::protocol::eventstream::{EventHeader, append_message};
    let payload = serde_json::json!({ "bytes": b64_payload });
    let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let headers = vec![
        EventHeader {
            name: ":event-type".to_string(),
            value: "chunk".to_string(),
        },
        EventHeader {
            name: ":content-type".to_string(),
            value: "application/json".to_string(),
        },
        EventHeader {
            name: ":message-type".to_string(),
            value: "event".to_string(),
        },
    ];
    let mut buf = Vec::with_capacity(64 + payload_bytes.len());
    append_message(&mut buf, &headers, &payload_bytes);
    buf
}

/// Fallback for InvokeModelWithResponseStream — re-uses the existing
/// buffered translator and converts the resulting marker-shaped
/// Value into a single-shot streaming response. Same content as
/// before, just delivered through the streaming pipeline so the
/// gateway uses chunked transfer.
fn buffered_stream_to_streaming(value: Value) -> HandlerResult {
    use awsim_core::protocol::eventstream::try_encode;
    let bytes = try_encode(&value).unwrap_or_else(|| {
        // Shouldn't happen — invoke_model_with_response_stream always
        // wraps in the marker — but we keep the response shape
        // sensible by encoding the value as-is if not.
        serde_json::to_vec(&value).unwrap_or_default()
    });
    let body: HandlerByteStream =
        stream::once(async move { Ok::<Bytes, AwsError>(Bytes::from(bytes)) }).boxed();
    HandlerResult::Streaming {
        body,
        content_type: "application/vnd.amazon.eventstream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_upstream_429_to_throttling() {
        let err = map_upstream_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "{\"error\":{\"message\":\"Rate limit reached\",\"code\":\"rate_limit_exceeded\"}}",
        );
        assert_eq!(err.code, "ThrottlingException");
        assert_eq!(err.status.as_u16(), 429);
        assert!(err.message.contains("Rate limit reached"));
    }

    #[test]
    fn map_upstream_403_to_access_denied() {
        let err = map_upstream_error(reqwest::StatusCode::FORBIDDEN, "denied");
        assert_eq!(err.code, "AccessDeniedException");
        assert_eq!(err.status.as_u16(), 403);
    }

    #[test]
    fn map_upstream_400_to_validation() {
        let err = map_upstream_error(reqwest::StatusCode::BAD_REQUEST, "bad input");
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn map_upstream_500_to_internal_server() {
        let err = map_upstream_error(reqwest::StatusCode::BAD_GATEWAY, "upstream down");
        assert_eq!(err.code, "InternalServerException");
        assert_eq!(err.status.as_u16(), 500);
    }

    #[test]
    fn map_upstream_truncates_huge_body() {
        let big = "x".repeat(2_000);
        let err = map_upstream_error(reqwest::StatusCode::TOO_MANY_REQUESTS, &big);
        // 1024 char limit + the leading "Bedrock backend returned " prefix
        // leaves us under ~1100 chars regardless of the upstream payload.
        assert!(err.message.len() < 1100, "got {} chars", err.message.len());
    }

    #[test]
    fn extract_body_accepts_json_string() {
        let input = json!({ "modelId": "x", "body": "{\"messages\":[]}" });
        let body = extract_body(&input).unwrap();
        assert!(body["messages"].is_array());
    }

    #[test]
    fn extract_body_accepts_object() {
        let input = json!({ "modelId": "x", "body": { "messages": [] } });
        let body = extract_body(&input).unwrap();
        assert!(body["messages"].is_array());
    }

    #[test]
    fn extract_body_rejects_garbage_string() {
        let input = json!({ "modelId": "x", "body": "not json" });
        let err = extract_body(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    /// Real SDK wire shape: the REST parser deserializes the HTTP
    /// body directly into the input and merges `modelId` from the
    /// path, so there is *no* `body` wrapper. The model body is
    /// whatever isn't a path param. Regression for
    /// `EmbeddingService` + KB indexing returning
    /// `ValidationException: inputText is required` because the
    /// old extract_body returned an empty object whenever the
    /// `body` field was absent.
    #[test]
    fn extract_body_unwraps_rest_shape_minus_path_params() {
        let input = json!({
            "modelId": "amazon.titan-embed-text-v2:0",
            "inputText": "hello world",
            "dimensions": 1024,
            "normalize": true,
        });
        let body = extract_body(&input).unwrap();
        assert_eq!(body["inputText"], "hello world");
        assert_eq!(body["dimensions"], 1024);
        assert_eq!(body["normalize"], true);
        // `modelId` is the path param, not part of the model body.
        assert!(body.get("modelId").is_none());
    }

    #[test]
    fn extract_body_is_empty_object_for_empty_input() {
        let input = json!({ "modelId": "amazon.titan-text-express-v1" });
        let body = extract_body(&input).unwrap();
        assert!(body.is_object());
        assert_eq!(body.as_object().unwrap().len(), 0);
    }

    #[test]
    fn stream_request_serializes_with_include_usage() {
        let req = openai::ChatRequest {
            model: "m".into(),
            stream: Some(true),
            stream_options: Some(openai::StreamOptions {
                include_usage: true,
            }),
            ..openai::ChatRequest::default()
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["stream"], true);
        assert_eq!(v["stream_options"]["include_usage"], true);
    }

    #[test]
    fn non_streaming_request_omits_stream_options() {
        let req = openai::ChatRequest {
            model: "m".into(),
            ..openai::ChatRequest::default()
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("stream").is_none());
        assert!(v.get("stream_options").is_none());
    }

    #[test]
    fn detects_string_content_fallback_signal() {
        let body = "{\"error\":{\"message\":\"messages[1].content must be a string\"}}";
        assert!(needs_string_content_fallback(body));
        assert!(!needs_string_content_fallback("rate limit reached"));
    }

    #[test]
    fn flattens_multimodal_parts_for_text_only_retry() {
        let mut req = openai::ChatRequest {
            model: "m".into(),
            messages: vec![openai::ChatMessage {
                role: "user".into(),
                content: openai::MessageContent::Parts(vec![
                    openai::ContentPart::Text {
                        text: "what is this?".into(),
                    },
                    openai::ContentPart::ImageUrl {
                        image_url: openai::ImageUrl {
                            url: "data:image/png;base64,AAA".into(),
                        },
                    },
                ]),
                ..openai::ChatMessage::default()
            }],
            ..openai::ChatRequest::default()
        };
        let changed = flatten_request_content(&mut req);
        assert!(changed);
        let text = match &req.messages[0].content {
            openai::MessageContent::Text(s) => s.clone(),
            other => panic!("expected text after flatten, got {other:?}"),
        };
        assert!(text.contains("what is this?"));
        assert!(text.contains("[Image attachment omitted"));
    }

    #[test]
    fn flatten_request_content_is_noop_on_text_only_messages() {
        let mut req = openai::ChatRequest {
            model: "m".into(),
            messages: vec![openai::ChatMessage {
                role: "user".into(),
                content: openai::MessageContent::text("hello"),
                ..openai::ChatMessage::default()
            }],
            ..openai::ChatRequest::default()
        };
        assert!(!flatten_request_content(&mut req));
    }

    /// OpenAI streams tool calls as fragments: `id` and `function.name`
    /// arrive once, then `function.arguments` trickles in across many
    /// chunks. The accumulator must stitch them back together by index.
    #[test]
    fn accumulate_sse_assembles_split_tool_call_fragments() {
        let raw = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tu_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Kathmandu\\\"}\"}}]}}]}\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\
data: [DONE]\n";
        let acc = accumulate_sse(raw);
        assert_eq!(acc.tool_calls.len(), 1);
        assert_eq!(acc.tool_calls[0].id, "tu_1");
        assert_eq!(acc.tool_calls[0].name, "get_weather");
        assert_eq!(acc.tool_calls[0].arguments, "{\"city\":\"Kathmandu\"}");
        assert_eq!(acc.finish_reason.as_deref(), Some("tool_calls"));
    }
}
