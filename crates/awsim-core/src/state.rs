use dashmap::DashMap;
use std::sync::Arc;

/// A thread-safe, account+region-namespaced state store.
///
/// Each AWS service uses this to store its state, ensuring that
/// resources in different accounts/regions are isolated.
///
/// Example:
/// ```ignore
/// let store = AccountRegionStore::<SqsState>::new();
/// let state = store.get("000000000000", "us-east-1");
/// state.queues.insert("my-queue".into(), queue);
/// ```
#[derive(Debug, Clone)]
pub struct AccountRegionStore<T: Default + Send + Sync + 'static> {
    inner: Arc<DashMap<(String, String), Arc<T>>>,
}

impl<T: Default + Send + Sync + 'static> AccountRegionStore<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Get or create the state for a given account+region pair.
    pub fn get(&self, account_id: &str, region: &str) -> Arc<T> {
        self.inner
            .entry((account_id.to_string(), region.to_string()))
            .or_insert_with(|| Arc::new(T::default()))
            .clone()
    }

    /// Clear all state (useful for testing).
    pub fn clear(&self) {
        self.inner.clear();
    }
}

impl<T: Default + Send + Sync + 'static> Default for AccountRegionStore<T> {
    fn default() -> Self {
        Self::new()
    }
}
