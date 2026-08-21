//! Per-request detail capture. The data that powers the UI's "Inspect"
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

#[derive(Debug, Clone)]
pub struct CapturedBody {
    /// The captured slice, or `None` when capture was skipped entirely
    /// (e.g. empty body).
    ///
    /// Held as raw [`Bytes`] and base64-encoded only when the entry is
    /// serialized. Capture happens on every request; serialization
    /// happens when somebody actually opens the inspect drawer, which
    /// is rare. Encoding eagerly would charge every request for a
    /// payload almost none of them ever display.
    data: Option<Bytes>,
    /// Total size of the original body before truncation.
    pub size: u64,
    /// True if the captured slice is shorter than `size`.
    pub truncated: bool,
}

impl CapturedBody {
    /// A no-content placeholder used for streaming responses, where
    /// there's nothing to capture. The body is forwarded chunk by
    /// chunk to the client without buffering. The inspect drawer
    /// renders the message as the body so users see *why* there's
    /// no data.
    pub fn placeholder(message: &str) -> Self {
        Self {
            data: Some(Bytes::copy_from_slice(message.as_bytes())),
            size: message.len() as u64,
            truncated: false,
        }
    }

    /// The captured bytes, or `None` when the body was empty.
    ///
    /// In-process consumers (request replay) should use this rather
    /// than round-tripping through the serialized base64 form.
    pub fn data(&self) -> Option<&Bytes> {
        self.data.as_ref()
    }
}

/// Serialized shape is unchanged from when the bytes were held
/// pre-encoded: a `data_b64` string field, null for an empty body. The
/// admin API and the UI both key off that name.
impl Serialize for CapturedBody {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("CapturedBody", 3)?;
        let encoded = self
            .data
            .as_ref()
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
        out.serialize_field("data_b64", &encoded)?;
        out.serialize_field("size", &self.size)?;
        out.serialize_field("truncated", &self.truncated)?;
        out.end()
    }
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

/// In-memory ring buffer of recent request details. Cheap to clone. The
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

    /// Newest-first list of recent detail ids. Used for "open last request".
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

/// Capture a body slice with a hard cap.
///
/// Cheap by construction: [`Bytes::slice`] is a refcount bump plus a
/// range, so capture copies no payload bytes at all. The base64 encode
/// that makes the capture work for binary payloads is deferred to
/// serialization time.
pub fn capture_body(body: &Bytes, cap: usize) -> CapturedBody {
    let size = body.len() as u64;
    if body.is_empty() {
        return CapturedBody {
            data: None,
            size: 0,
            truncated: false,
        };
    }
    let truncated = body.len() > cap;
    let data = if truncated {
        body.slice(0..cap)
    } else {
        body.clone()
    };
    CapturedBody {
        data: Some(data),
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
                request_body: capture_body(&Bytes::new(), 32),
                response_body: capture_body(&Bytes::new(), 32),
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
        assert_eq!(captured.data().unwrap().len(), 40);
    }

    #[test]
    fn empty_body_yields_none() {
        let captured = capture_body(&Bytes::new(), 64);
        assert!(captured.data().is_none());
        assert!(!captured.truncated);
    }

    /// The UI and the admin replay endpoint both read `data_b64` off the
    /// serialized form. Deferring the encode must not change the wire
    /// shape.
    #[test]
    fn serialized_shape_stays_base64() {
        let captured = capture_body(&Bytes::from_static(b"hello"), 64);
        let json = serde_json::to_value(&captured).unwrap();
        assert_eq!(json["data_b64"], "aGVsbG8=");
        assert_eq!(json["size"], 5);
        assert_eq!(json["truncated"], false);

        let empty = serde_json::to_value(capture_body(&Bytes::new(), 64)).unwrap();
        assert!(empty["data_b64"].is_null());
        assert_eq!(empty["size"], 0);
    }
}
