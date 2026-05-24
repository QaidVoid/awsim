//! TTL-bounded cache for AWS idempotency tokens.
//!
//! Many AWS operations accept a client-supplied token (`ClientToken`,
//! `IdempotencyToken`, `ClientRequestToken`, `CreatorRequestId`) so
//! that a retry of the same request returns the same result instead
//! of creating a duplicate resource. AWS distinguishes three cases:
//!
//! 1. New token: run the operation, cache the result keyed by token
//!    plus a hash of the request parameters.
//! 2. Repeat with the same parameters: return the cached result.
//! 3. Repeat with different parameters under the same token: return
//!    `IdempotencyParameterMismatchException` (or a
//!    service-specific variant).
//!
//! The cache evicts entries past their TTL on read or via the
//! periodic [`Self::sweep`] call. Per-service TTLs vary
//! (DynamoDB TransactWriteItems is 10 minutes; ACM RequestCertificate
//! is 1 hour; most are 24 hours), so the cache is parameterised on
//! construction.

use crate::error::AwsError;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default TTL for idempotency tokens (24 hours). Override per
/// service via [`IdempotencyCache::with_ttl`].
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Outcome of a [`IdempotencyCache::lookup`] call.
#[derive(Debug)]
pub enum Lookup<V> {
    /// No prior call seen for this token; the caller should run the
    /// operation and then call [`IdempotencyCache::insert`] with the
    /// result.
    Miss,
    /// Same token + same params seen before; replay the cached
    /// value.
    Hit(V),
    /// Same token, different params. The caller should surface
    /// this as the appropriate service-specific exception.
    Mismatch,
}

#[derive(Debug, Clone)]
struct Entry<V> {
    request_hash: u64,
    value: V,
    inserted_at: Instant,
}

/// In-memory idempotency cache.
///
/// Cloning the cache shares the underlying store; copies hand out
/// the same view. Safe for concurrent access.
#[derive(Debug)]
pub struct IdempotencyCache<V: Clone> {
    inner: Mutex<HashMap<String, Entry<V>>>,
    ttl: Duration,
}

impl<V: Clone> IdempotencyCache<V> {
    /// Create a cache with the default 24h TTL.
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// Create a cache with a service-specific TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Look up a token. Returns one of [`Miss`], [`Hit`], or
    /// [`Mismatch`]. Expired entries are removed in passing.
    pub fn lookup(&self, token: &str, request_hash: u64) -> Lookup<V> {
        let mut g = self.inner.lock().unwrap();
        if let Some(entry) = g.get(token) {
            if entry.inserted_at.elapsed() > self.ttl {
                g.remove(token);
                return Lookup::Miss;
            }
            return if entry.request_hash == request_hash {
                Lookup::Hit(entry.value.clone())
            } else {
                Lookup::Mismatch
            };
        }
        Lookup::Miss
    }

    /// Record the result of a successful operation against `token`.
    /// Overwrites any prior entry for the same token (callers should
    /// only call this after a [`Lookup::Miss`]).
    pub fn insert(&self, token: impl Into<String>, request_hash: u64, value: V) {
        let token = token.into();
        let mut g = self.inner.lock().unwrap();
        g.insert(
            token,
            Entry {
                request_hash,
                value,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Drop entries past their TTL. Call from the tick loop.
    pub fn sweep(&self) {
        let ttl = self.ttl;
        let mut g = self.inner.lock().unwrap();
        g.retain(|_, e| e.inserted_at.elapsed() <= ttl);
    }

    /// Number of live entries. Surfaced for diagnostics / tests.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// True when no entries are cached.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<V: Clone> Default for IdempotencyCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate a client-supplied idempotency token.
///
/// AWS tokens are 1-64 visible ASCII characters (the documented
/// regex is `^[!-~]+$`). Returns a `ValidationException` on shape
/// violations so the caller can `?` it from the handler.
pub fn validate_token(token: &str) -> Result<(), AwsError> {
    if token.is_empty() || token.len() > 64 {
        return Err(AwsError::validation(
            "ClientToken must be 1-64 characters long.",
        ));
    }
    if !token.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(AwsError::validation(
            "ClientToken must contain only printable ASCII characters.",
        ));
    }
    Ok(())
}

/// Hash a request body (or canonical params) into a [`u64`] for use
/// as the `request_hash` argument to [`IdempotencyCache::lookup`].
///
/// Uses a non-cryptographic hasher: the value is only ever
/// compared for equality against an earlier hash from the same
/// process, so collision resistance is not load-bearing.
pub fn hash_request<H: std::hash::Hash>(value: &H) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn miss_on_unseen_token() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        assert!(matches!(cache.lookup("tok-1", 0), Lookup::Miss));
    }

    #[test]
    fn hit_on_same_token_and_params() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        cache.insert("tok-1", 42, "first-result".to_string());
        let result = cache.lookup("tok-1", 42);
        assert!(matches!(result, Lookup::Hit(ref v) if v == "first-result"));
    }

    #[test]
    fn mismatch_on_same_token_different_params() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        cache.insert("tok-1", 42, "first".to_string());
        assert!(matches!(cache.lookup("tok-1", 999), Lookup::Mismatch));
    }

    #[test]
    fn expired_entry_treated_as_miss() {
        let cache: IdempotencyCache<String> = IdempotencyCache::with_ttl(Duration::from_millis(5));
        cache.insert("tok-1", 1, "x".into());
        sleep(Duration::from_millis(20));
        assert!(matches!(cache.lookup("tok-1", 1), Lookup::Miss));
    }

    #[test]
    fn sweep_drops_expired_entries() {
        let cache: IdempotencyCache<String> = IdempotencyCache::with_ttl(Duration::from_millis(5));
        cache.insert("a", 1, "x".into());
        cache.insert("b", 2, "y".into());
        sleep(Duration::from_millis(20));
        cache.sweep();
        assert!(cache.is_empty());
    }

    #[test]
    fn validate_token_accepts_printable_ascii() {
        validate_token("abc-123_XYZ").unwrap();
        validate_token("!~").unwrap();
    }

    #[test]
    fn validate_token_rejects_empty() {
        assert!(validate_token("").is_err());
    }

    #[test]
    fn validate_token_rejects_over_64_chars() {
        let long: String = "a".repeat(65);
        assert!(validate_token(&long).is_err());
    }

    #[test]
    fn validate_token_rejects_control_chars() {
        assert!(validate_token("with\tspace").is_err());
        assert!(validate_token("with space").is_err());
        assert!(validate_token("with\ncontrol").is_err());
    }

    #[test]
    fn hash_request_stable_across_calls() {
        let a = ("CreateUser", "alice", 42u32);
        assert_eq!(hash_request(&a), hash_request(&a));
        let b = ("CreateUser", "bob", 42u32);
        assert_ne!(hash_request(&a), hash_request(&b));
    }
}
