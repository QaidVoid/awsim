use dashmap::DashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
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
#[derive(Debug)]
pub struct AccountRegionStore<T: Default + Send + Sync + 'static> {
    inner: Arc<DashMap<(String, String), Arc<T>>>,
}

impl<T: Default + Send + Sync + 'static> Clone for AccountRegionStore<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
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

    /// Iterate over all (account_id, region) -> state entries.
    ///
    /// Returns a snapshot of the keys paired with the `Arc<T>` values so the
    /// caller can read state without holding any DashMap locks long-term.
    pub fn iter_all(&self) -> Vec<((String, String), Arc<T>)> {
        self.inner
            .iter()
            .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
            .collect()
    }

    /// Insert a state value for the given (account_id, region), replacing any
    /// existing entry.
    pub fn set(&self, account_id: &str, region: &str, value: T) {
        self.inner.insert(
            (account_id.to_string(), region.to_string()),
            Arc::new(value),
        );
    }
}

pub trait Snapshottable: Send + Sync + Sized {
    type Snapshot: Serialize + DeserializeOwned + Send;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot;

    fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self);
}

impl<T: Snapshottable + Default + 'static> AccountRegionStore<T> {
    pub fn snapshot_to_bytes(&self) -> Option<Vec<u8>> {
        let snaps: Vec<T::Snapshot> = self
            .iter_all()
            .into_iter()
            .map(|((acct, region), state)| state.to_snapshot(&acct, &region))
            .collect();
        serde_json::to_vec(&snaps).ok()
    }

    pub fn restore_from_bytes(&self, data: &[u8]) -> Result<(), String> {
        let snaps: Vec<T::Snapshot> = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        self.clear();
        for snap in snaps {
            let (acct, region, state) = T::from_snapshot(snap);
            self.set(&acct, &region, state);
        }
        Ok(())
    }
}

impl<T: Default + Send + Sync + 'static> Default for AccountRegionStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// One (account, region) pair's state, as read back from a snapshot.
#[derive(serde::Deserialize)]
pub struct StoreBucket<T> {
    pub account_id: String,
    pub region: String,
    pub state: T,
}

/// Write-side mirror of [`StoreBucket`] that borrows the live state, so
/// snapshotting does not clone every service's entire state map.
#[derive(Serialize)]
struct StoreBucketRef<'a, T> {
    account_id: &'a str,
    region: &'a str,
    state: &'a T,
}

/// Serialise an entire [`AccountRegionStore`] whose state type derives
/// `Serialize` directly.
///
/// Most services keep their state in `DashMap` fields that serialise as
/// they are, so implementing [`Snapshottable`] with a hand-written mirror
/// type buys nothing beyond a second place to forget a field. Prefer this
/// where a direct derive is possible; reach for `Snapshottable` only when
/// the on-disk shape must differ from the in-memory one.
pub fn snapshot_store<T>(store: &AccountRegionStore<T>) -> Option<Vec<u8>>
where
    T: Serialize + Default + Send + Sync + 'static,
{
    let entries = store.iter_all();
    let buckets: Vec<StoreBucketRef<'_, T>> = entries
        .iter()
        .map(|((account_id, region), state)| StoreBucketRef {
            account_id,
            region,
            state: state.as_ref(),
        })
        .collect();
    serde_json::to_vec(&buckets).ok()
}

/// Restore an [`AccountRegionStore`] previously written by
/// [`snapshot_store`], replacing any existing contents.
pub fn restore_store<T>(store: &AccountRegionStore<T>, data: &[u8]) -> Result<(), String>
where
    T: DeserializeOwned + Default + Send + Sync + 'static,
{
    let buckets: Vec<StoreBucket<T>> = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    store.clear();
    for bucket in buckets {
        store.set(&bucket.account_id, &bucket.region, bucket.state);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug, Default)]
    struct TestState {
        value: AtomicU32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct TestSnapshot {
        account_id: String,
        region: String,
        value: u32,
    }

    impl Snapshottable for TestState {
        type Snapshot = TestSnapshot;

        fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
            TestSnapshot {
                account_id: account_id.to_string(),
                region: region.to_string(),
                value: self.value.load(Ordering::SeqCst),
            }
        }

        fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self) {
            (
                snapshot.account_id,
                snapshot.region,
                TestState {
                    value: AtomicU32::new(snapshot.value),
                },
            )
        }
    }

    #[test]
    fn snapshot_round_trip() {
        let store = AccountRegionStore::<TestState>::new();
        store
            .get("111", "us-east-1")
            .value
            .store(7, Ordering::SeqCst);
        store
            .get("222", "us-west-2")
            .value
            .store(42, Ordering::SeqCst);

        let bytes = store.snapshot_to_bytes().expect("snapshot");

        let restored = AccountRegionStore::<TestState>::new();
        restored.restore_from_bytes(&bytes).expect("restore");

        let mut entries: Vec<((String, String), u32)> = restored
            .iter_all()
            .into_iter()
            .map(|(k, v)| (k, v.value.load(Ordering::SeqCst)))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            entries,
            vec![
                (("111".to_string(), "us-east-1".to_string()), 7),
                (("222".to_string(), "us-west-2".to_string()), 42),
            ]
        );
    }

    #[test]
    fn restore_replaces_existing_state() {
        let store = AccountRegionStore::<TestState>::new();
        store
            .get("111", "us-east-1")
            .value
            .store(7, Ordering::SeqCst);

        let bytes = store.snapshot_to_bytes().expect("snapshot");

        store
            .get("111", "us-east-1")
            .value
            .store(99, Ordering::SeqCst);
        store
            .get("999", "eu-west-1")
            .value
            .store(1, Ordering::SeqCst);

        store.restore_from_bytes(&bytes).expect("restore");

        let entries = store.iter_all();
        assert_eq!(entries.len(), 1);
        let ((acct, region), state) = &entries[0];
        assert_eq!(acct, "111");
        assert_eq!(region, "us-east-1");
        assert_eq!(state.value.load(Ordering::SeqCst), 7);
    }
}
