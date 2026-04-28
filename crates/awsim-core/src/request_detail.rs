//! Per-request detail capture — the data that powers the UI's "Inspect"
//! drawer. Stored in a bounded ring buffer keyed by request id, so the
//! UI can pull headers / bodies on demand without bloating the SSE stream.
//!
//! Bodies are size-capped (default 64 KiB each) to avoid runaway memory
//! when callers upload large objects (S3 PUT, ECR layer push, etc.).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use axum::http::HeaderMap;
use base64::Engine;
use bytes::Bytes;
use serde::Serialize;

/// Default per-body capture cap (64 KiB each direction).
pub const DEFAULT_BODY_CAP: usize = 64 * 1024;
/// Default ring-buffer capacity (number of detail entries kept in memory).
pub const DEFAULT_RING_CAPACITY: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct CapturedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapturedBody {
    /// `Some` when the body fit the cap (or was truncated to it). `None`
    /// when capture was skipped entirely (e.g. empty body).
    pub data_b64: Option<String>,
    /// Total size of the original body before truncation.
    pub size: u64,
    /// True if the captured slice is shorter than `size`.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestDetail {
    pub id: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub status_code: u16,
    pub request_headers: Vec<CapturedHeader>,
    pub response_headers: Vec<CapturedHeader>,
    pub request_body: CapturedBody,
    pub response_body: CapturedBody,
}

/// In-memory ring buffer of recent request details. Cheap to clone — the
/// backing store is behind an `Arc<Mutex<_>>`.
#[derive(Clone)]
pub struct RequestDetailStore {
    inner: Arc<Mutex<RequestDetailInner>>,
    cap: usize,
    body_cap: usize,
}

struct RequestDetailInner {
    order: VecDeque<String>,
    map: HashMap<String, RequestDetail>,
}

impl RequestDetailStore {
    pub fn new(cap: usize, body_cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RequestDetailInner {
                order: VecDeque::with_capacity(cap),
                map: HashMap::with_capacity(cap),
            })),
            cap,
            body_cap,
        }
    }

    pub fn body_cap(&self) -> usize {
        self.body_cap
    }

    /// Insert a detail entry, evicting the oldest if we're at capacity.
    pub fn insert(&self, detail: RequestDetail) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(), // recover from poison
        };
        while inner.order.len() >= self.cap {
            if let Some(old) = inner.order.pop_front() {
                inner.map.remove(&old);
            } else {
                break;
            }
        }
        inner.order.push_back(detail.id.clone());
        inner.map.insert(detail.id.clone(), detail);
    }

    pub fn get(&self, id: &str) -> Option<RequestDetail> {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.map.get(id).cloned()
    }

    /// Newest-first list of recent detail ids — used for "open last request".
    pub fn recent_ids(&self, n: usize) -> Vec<String> {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.order.iter().rev().take(n).cloned().collect()
    }
}

impl Default for RequestDetailStore {
    fn default() -> Self {
        Self::new(DEFAULT_RING_CAPACITY, DEFAULT_BODY_CAP)
    }
}

/// Convert an axum `HeaderMap` into a clone-friendly captured-header list.
pub fn capture_headers(headers: &HeaderMap) -> Vec<CapturedHeader> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|v| CapturedHeader {
                name: name.as_str().to_string(),
                value: v.to_string(),
            })
        })
        .collect()
}

/// Capture a body slice with a hard cap, base64-encoding the bytes so the
/// store works for both text and binary payloads.
pub fn capture_body(body: &Bytes, cap: usize) -> CapturedBody {
    let size = body.len() as u64;
    if body.is_empty() {
        return CapturedBody {
            data_b64: None,
            size: 0,
            truncated: false,
        };
    }
    let truncated = body.len() > cap;
    let slice = if truncated { &body[..cap] } else { &body[..] };
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(slice);
    CapturedBody {
        data_b64: Some(data_b64),
        size,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn ring_evicts_in_fifo() {
        let store = RequestDetailStore::new(2, 32);
        for i in 0..3 {
            let id = format!("r{i}");
            store.insert(RequestDetail {
                id: id.clone(),
                method: "GET".into(),
                path: "/".into(),
                query: None,
                status_code: 200,
                request_headers: vec![],
                response_headers: vec![],
                request_body: CapturedBody {
                    data_b64: None,
                    size: 0,
                    truncated: false,
                },
                response_body: CapturedBody {
                    data_b64: None,
                    size: 0,
                    truncated: false,
                },
            });
        }
        assert!(store.get("r0").is_none(), "oldest evicted");
        assert!(store.get("r1").is_some());
        assert!(store.get("r2").is_some());
    }

    #[test]
    fn body_truncates_at_cap() {
        let body = Bytes::from(vec![0u8; 100]);
        let captured = capture_body(&body, 40);
        assert!(captured.truncated);
        assert_eq!(captured.size, 100);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(captured.data_b64.unwrap())
            .unwrap();
        assert_eq!(decoded.len(), 40);
    }

    #[test]
    fn empty_body_yields_none() {
        let captured = capture_body(&Bytes::new(), 64);
        assert!(captured.data_b64.is_none());
        assert!(!captured.truncated);
    }
}
