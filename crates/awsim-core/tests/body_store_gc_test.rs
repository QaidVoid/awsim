use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{BlobInventory, BodyStore};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("awsim-bs-it-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct StubInventory {
    triples: Vec<(String, String, String)>,
}

impl BlobInventory for StubInventory {
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        self.triples.clone()
    }
}

#[test]
fn end_to_end_gc_removes_orphans_only() {
    let root = tmp_dir("e2e");
    let store = BodyStore::new(root.clone());

    store.write_blob("objects", "alpha", "k1", b"AAA").unwrap();
    store.write_blob("objects", "alpha", "k2", b"BB").unwrap();
    store
        .write_blob("objects", "beta", "deep/nested/k3", b"CCCC")
        .unwrap();
    store
        .write_blob("objects", "ghost", "leftover", b"X")
        .unwrap();

    let inventory = StubInventory {
        triples: vec![
            ("objects".to_string(), "alpha".to_string(), "k1".to_string()),
            (
                "objects".to_string(),
                "beta".to_string(),
                "deep/nested/k3".to_string(),
            ),
        ],
    };
    let known: HashSet<_> = inventory.known_blobs().into_iter().collect();

    let (deleted, freed) = store.gc_orphaned(&["objects"], &known).unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(freed, 2 + 1);

    assert_eq!(store.read_blob("objects", "alpha", "k1").unwrap(), b"AAA");
    assert_eq!(
        store
            .read_blob("objects", "beta", "deep/nested/k3")
            .unwrap(),
        b"CCCC"
    );
    assert!(store.read_blob("objects", "alpha", "k2").is_err());
    assert!(!store.group_dir("objects").join("ghost").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn empty_inventory_clears_groups_and_dirs() {
    let root = tmp_dir("empty");
    let store = BodyStore::new(root.clone());

    store.write_blob("sqs", "q1", "m1", b"hi").unwrap();
    store.write_blob("sqs", "q1", "m2", b"there").unwrap();
    store.write_blob("sqs", "q2", "m3", b"go").unwrap();

    let known: HashSet<(String, String, String)> = HashSet::new();
    let (deleted, freed) = store.gc_orphaned(&["sqs"], &known).unwrap();

    assert_eq!(deleted, 3);
    assert_eq!(freed, 2 + 5 + 2);
    assert!(!store.group_dir("sqs").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn gc_skips_groups_outside_filter() {
    let root = tmp_dir("filter");
    let store = BodyStore::new(root.clone());

    store
        .write_blob("lambda", "fn1", "$LATEST", b"code")
        .unwrap();
    store
        .write_blob("ecr", "repo", "sha256:abc", b"layer")
        .unwrap();

    let known: HashSet<(String, String, String)> = HashSet::new();
    let (deleted, _freed) = store.gc_orphaned(&["lambda"], &known).unwrap();
    assert_eq!(deleted, 1);
    assert!(!store.group_dir("lambda").exists());
    assert_eq!(
        store.read_blob("ecr", "repo", "sha256:abc").unwrap(),
        b"layer"
    );

    let _ = std::fs::remove_dir_all(&root);
}
