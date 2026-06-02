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
use std::sync::{Arc, Mutex};
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
    /// Per-token guards so concurrent calls with the same token
    /// serialize: the first runs `compute`, later callers wait on the
    /// guard and then observe the cached result. Idle guards (held only
    /// by this map) are reclaimed in [`Self::sweep`].
    in_flight: Mutex<HashMap<String, Arc<Mutex<()>>>>,
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
            in_flight: Mutex::new(HashMap::new()),
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

    /// Convenience around [`Self::lookup`] + [`Self::insert`].
    ///
    /// Cache hit: return the stored value without invoking `compute`.
    /// Mismatch: return `IdempotencyParameterMismatchException` so the
    /// caller can `?` it from a handler.
    /// Miss: run `compute`, cache its successful result, and return it.
    /// Errors from `compute` are propagated unchanged and not cached
    /// (AWS only replays successful results on retry).
    ///
    /// Concurrent calls with the same token are serialized: the first
    /// runs `compute`; later callers block on a per-token guard and,
    /// once it is released, observe the first caller's cached result
    /// instead of recomputing. This matches AWS, which replays the
    /// original result for an in-flight token rather than running the
    /// side effect twice.
    ///
    /// Deadlock note: `compute` must not re-enter `lookup_or_insert`
    /// for the same token on the same thread - it would block on the
    /// guard it already holds.
    pub fn lookup_or_insert<F>(
        &self,
        token: &str,
        request_hash: u64,
        compute: F,
    ) -> Result<V, AwsError>
    where
        F: FnOnce() -> Result<V, AwsError>,
    {
        // Fast path: a settled hit or mismatch needs no guard.
        match self.lookup(token, request_hash) {
            Lookup::Hit(v) => return Ok(v),
            Lookup::Mismatch => return Err(mismatch_error()),
            Lookup::Miss => {}
        }

        // Block on the per-token guard so a concurrent first caller
        // finishes its compute + insert before we proceed.
        let guard = {
            let mut g = self.in_flight.lock().unwrap();
            Arc::clone(
                g.entry(token.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _held = guard.lock().unwrap();

        // Double-check: the winner may have inserted while we waited.
        match self.lookup(token, request_hash) {
            Lookup::Hit(v) => Ok(v),
            Lookup::Mismatch => Err(mismatch_error()),
            Lookup::Miss => {
                let value = compute()?;
                self.insert(token, request_hash, value.clone());
                Ok(value)
            }
        }
    }

    /// Drop entries past their TTL, and reclaim idle per-token guards.
    /// Call from the tick loop.
    pub fn sweep(&self) {
        let ttl = self.ttl;
        self.inner
            .lock()
            .unwrap()
            .retain(|_, e| e.inserted_at.elapsed() <= ttl);
        // A guard with a strong count of 1 is held only by this map, so
        // no caller is parked on it and it is safe to drop.
        self.in_flight
            .lock()
            .unwrap()
            .retain(|_, g| Arc::strong_count(g) > 1);
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

/// The error returned when a token is replayed with different request
/// parameters than the original call.
fn mismatch_error() -> AwsError {
    AwsError::bad_request(
        "IdempotencyParameterMismatchException",
        "Request parameters do not match those used in a prior call with the same ClientToken.",
    )
}

/// Idempotency cache scoped per `(account_id, region)`.
///
/// AWS idempotency tokens are namespaced to the account that issued
/// the request: two accounts using the same `ClientToken` on the
/// same operation must not collide, and a token minted in one
/// region must not satisfy a retry sent to another. This wrapper
/// lazily creates a fresh [`IdempotencyCache`] per scope on first
/// touch so every service consuming idempotent creates can simply
/// keep an `AccountRegionIdempotencyCache` field and call
/// [`Self::scope`] from the handler.
///
/// The TTL set on construction applies to every scope.
/// Outer-map type alias kept readable for the `Mutex` wrapper; the
/// raw form trips clippy's `type_complexity` lint.
type ScopeMap<V> = HashMap<(String, String), Arc<IdempotencyCache<V>>>;

#[derive(Debug)]
pub struct AccountRegionIdempotencyCache<V: Clone> {
    inner: Mutex<ScopeMap<V>>,
    ttl: Duration,
}

impl<V: Clone> AccountRegionIdempotencyCache<V> {
    /// Create a per-scope cache with the default 24h TTL.
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// Create a per-scope cache with a service-specific TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Return the underlying cache for `(account_id, region)`,
    /// creating it on first touch. Callers can then drive
    /// [`IdempotencyCache::lookup_or_insert`] / `lookup` / `insert`
    /// against the returned handle.
    pub fn scope(&self, account_id: &str, region: &str) -> Arc<IdempotencyCache<V>> {
        let key = (account_id.to_string(), region.to_string());
        let mut g = self.inner.lock().unwrap();
        g.entry(key)
            .or_insert_with(|| Arc::new(IdempotencyCache::with_ttl(self.ttl)))
            .clone()
    }

    /// Drop expired entries across every scope. Call from the tick
    /// loop. Empty scopes are kept (the lazy `scope` call is cheap
    /// either way; the overhead of churning the outer map under load
    /// is not worth the byte savings).
    pub fn sweep(&self) {
        let scopes: Vec<Arc<IdempotencyCache<V>>> =
            self.inner.lock().unwrap().values().cloned().collect();
        for s in scopes {
            s.sweep();
        }
    }

    /// Total number of cached entries across every scope. Surfaced
    /// for diagnostics / tests.
    pub fn total_len(&self) -> usize {
        let scopes: Vec<Arc<IdempotencyCache<V>>> =
            self.inner.lock().unwrap().values().cloned().collect();
        scopes.iter().map(|s| s.len()).sum()
    }
}

impl<V: Clone> Default for AccountRegionIdempotencyCache<V> {
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
    fn concurrent_same_token_computes_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let cache: Arc<IdempotencyCache<u64>> = Arc::new(IdempotencyCache::new());
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let computes = Arc::clone(&computes);
                std::thread::spawn(move || {
                    cache
                        .lookup_or_insert("shared-token", 1, || {
                            computes.fetch_add(1, Ordering::SeqCst);
                            // Hold the per-token guard long enough that
                            // the other threads pile up behind it.
                            sleep(Duration::from_millis(30));
                            Ok(7u64)
                        })
                        .unwrap()
                })
            })
            .collect();

        let results: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Exactly one thread ran compute; the rest observed its result.
        assert_eq!(computes.load(Ordering::SeqCst), 1);
        assert!(results.iter().all(|&v| v == 7));
        assert_eq!(cache.len(), 1);

        // The idle guard is reclaimed on sweep.
        cache.sweep();
        assert_eq!(cache.len(), 1);
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
    fn lookup_or_insert_runs_compute_on_miss() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        let result = cache
            .lookup_or_insert("tok", 7, || Ok("computed".to_string()))
            .unwrap();
        assert_eq!(result, "computed");
        // Second call hits the cache without running compute.
        let result = cache
            .lookup_or_insert("tok", 7, || panic!("compute must not run on hit"))
            .unwrap();
        assert_eq!(result, "computed");
    }

    #[test]
    fn lookup_or_insert_returns_mismatch_exception() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        cache.insert("tok", 1, "first".into());
        let err = cache
            .lookup_or_insert("tok", 2, || Ok("second".to_string()))
            .unwrap_err();
        assert_eq!(err.code, "IdempotencyParameterMismatchException");
    }

    #[test]
    fn lookup_or_insert_does_not_cache_compute_errors() {
        let cache: IdempotencyCache<String> = IdempotencyCache::new();
        let err = cache
            .lookup_or_insert("tok", 1, || Err(AwsError::validation("boom")))
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        // Token is still a miss; a retry with valid compute succeeds.
        let result = cache
            .lookup_or_insert("tok", 1, || Ok("ok".to_string()))
            .unwrap();
        assert_eq!(result, "ok");
    }

    #[test]
    fn account_region_cache_isolates_scopes() {
        let cache: AccountRegionIdempotencyCache<String> = AccountRegionIdempotencyCache::new();
        let a = cache.scope("111111111111", "us-east-1");
        let b = cache.scope("222222222222", "us-east-1");
        a.insert("tok", 1, "alice".to_string());
        // Same token in a different account is a miss, not a hit on
        // alice's cached result.
        assert!(matches!(b.lookup("tok", 1), Lookup::Miss));
        assert!(matches!(a.lookup("tok", 1), Lookup::Hit(ref v) if v == "alice"));
    }

    #[test]
    fn account_region_cache_isolates_regions() {
        let cache: AccountRegionIdempotencyCache<String> = AccountRegionIdempotencyCache::new();
        let east = cache.scope("111111111111", "us-east-1");
        let west = cache.scope("111111111111", "us-west-2");
        east.insert("tok", 7, "east-only".to_string());
        assert!(matches!(west.lookup("tok", 7), Lookup::Miss));
    }

    #[test]
    fn account_region_cache_returns_same_handle_per_scope() {
        let cache: AccountRegionIdempotencyCache<String> = AccountRegionIdempotencyCache::new();
        let first = cache.scope("111111111111", "us-east-1");
        first.insert("tok", 1, "v".into());
        let second = cache.scope("111111111111", "us-east-1");
        // Repeated scope() returns a clone of the same Arc, so
        // entries inserted via `first` are visible via `second`.
        assert!(matches!(second.lookup("tok", 1), Lookup::Hit(ref v) if v == "v"));
    }

    #[test]
    fn account_region_cache_sweep_clears_every_scope() {
        let cache: AccountRegionIdempotencyCache<String> =
            AccountRegionIdempotencyCache::with_ttl(Duration::from_millis(5));
        cache
            .scope("a", "us-east-1")
            .insert("t1", 1, "x".to_string());
        cache
            .scope("b", "us-west-2")
            .insert("t2", 2, "y".to_string());
        assert_eq!(cache.total_len(), 2);
        sleep(Duration::from_millis(20));
        cache.sweep();
        assert_eq!(cache.total_len(), 0);
    }

    #[test]
    fn hash_request_stable_across_calls() {
        let a = ("CreateUser", "alice", 42u32);
        assert_eq!(hash_request(&a), hash_request(&a));
        let b = ("CreateUser", "bob", 42u32);
        assert_ne!(hash_request(&a), hash_request(&b));
    }
}
