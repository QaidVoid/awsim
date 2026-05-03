use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use awsim_core::{Body, BodyStore, Snapshottable};

/// Versioning status for a bucket.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VersioningStatus {
    #[default]
    Disabled,
    Enabled,
    Suspended,
}

/// A single notification destination (SQS, SNS, or Lambda).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDestination {
    /// "sqs", "sns", or "lambda"
    pub dest_type: String,
    pub arn: String,
    /// Event name filter prefix, e.g. "s3:ObjectCreated:*"
    pub events: Vec<String>,
}

/// Notification configuration for a bucket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationConfiguration {
    pub destinations: Vec<NotificationDestination>,
}

impl VersioningStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "",
            Self::Enabled => "Enabled",
            Self::Suspended => "Suspended",
        }
    }
}

/// An S3 bucket.
#[derive(Debug)]
pub struct Bucket {
    pub name: String,
    pub region: String,
    pub created_at: String,
    pub versioning: VersioningStatus,
    pub tags: HashMap<String, String>,
    pub policy: Option<String>,
    pub cors: Option<String>,
    /// Notification configuration (PutBucketNotificationConfiguration).
    pub notification_config: NotificationConfiguration,
    /// ACL configuration (stored as raw XML string).
    pub acl: Option<String>,
    /// Lifecycle configuration (stored as raw JSON-serialized value).
    pub lifecycle: Option<String>,
    /// Server-side encryption configuration (stored as raw JSON-serialized value).
    pub encryption: Option<String>,
    /// Logging configuration (stored as raw JSON-serialized value).
    pub logging: Option<String>,
    /// Generic named configs (website, replication, requestpayment, accelerate, etc.)
    /// keyed by config name → JSON string.
    pub configs: HashMap<String, String>,
    /// Objects keyed by object key — each value carries the full version
    /// history for that key, in chronological order.
    pub objects: DashMap<String, ObjectVersions>,
    /// Multipart uploads keyed by upload ID.
    pub multipart_uploads: DashMap<String, MultipartUpload>,
}

impl Bucket {
    pub fn new(
        name: impl Into<String>,
        region: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            region: region.into(),
            created_at: created_at.into(),
            versioning: VersioningStatus::Disabled,
            tags: HashMap::new(),
            policy: None,
            cors: None,
            notification_config: NotificationConfiguration::default(),
            acl: None,
            lifecycle: None,
            encryption: None,
            logging: None,
            configs: HashMap::new(),
            objects: DashMap::new(),
            multipart_uploads: DashMap::new(),
        }
    }
}

/// An S3 object stored in a bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Object {
    pub key: String,
    /// Object body — either in memory or backed by disk.
    pub body: Body,
    pub content_type: String,
    pub content_length: u64,
    /// MD5 hex digest wrapped in quotes, e.g. `"d41d8cd98f00b204e9800998ecf8427e"`.
    pub etag: String,
    /// RFC 7231 date string.
    pub last_modified: String,
    /// User-defined metadata from x-amz-meta-* headers.
    pub metadata: HashMap<String, String>,
    pub version_id: Option<String>,
    /// Object tags (key → value).
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub content_encoding: Option<String>,
    #[serde(default)]
    pub cache_control: Option<String>,
    #[serde(default)]
    pub content_disposition: Option<String>,
    #[serde(default)]
    pub content_language: Option<String>,
    #[serde(default)]
    pub expires: Option<String>,
    /// True when this entry is a delete marker — a tombstone written when
    /// DeleteObject lands on a versioning-enabled bucket without a VersionId.
    /// Delete markers carry a version_id but no body, and reads against them
    /// surface as NoSuchKey + `x-amz-delete-marker: true`.
    #[serde(default)]
    pub is_delete_marker: bool,
}

/// All versions of a single key within a bucket, in chronological order.
///
/// The last entry is the "current" version per S3 semantics. Versioning is
/// not "on" until the bucket transitions to Enabled — Disabled buckets keep
/// at most one entry with `version_id = None`. Suspended buckets keep prior
/// versions but new writes overwrite the single `null`-version slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectVersions {
    pub versions: Vec<S3Object>,
}

impl ObjectVersions {
    /// The most recent entry, regardless of whether it's a delete marker.
    pub fn latest(&self) -> Option<&S3Object> {
        self.versions.last()
    }

    /// The most recent non-delete-marker entry — what GetObject returns
    /// when no VersionId is supplied. `None` when every version is a DM.
    pub fn current(&self) -> Option<&S3Object> {
        match self.versions.last() {
            Some(o) if !o.is_delete_marker => Some(o),
            _ => None,
        }
    }

    /// Mutable view of the current entry — for header-only mutations such
    /// as PutObjectTagging that update the latest version's metadata in
    /// place rather than producing a new version.
    pub fn current_mut(&mut self) -> Option<&mut S3Object> {
        match self.versions.last_mut() {
            Some(o) if !o.is_delete_marker => Some(o),
            _ => None,
        }
    }

    /// Look up a specific version by ID. Treats `"null"` as the slot used
    /// for Disabled / Suspended writes (which have `version_id = None`).
    pub fn find(&self, version_id: &str) -> Option<&S3Object> {
        if version_id == "null" {
            self.versions.iter().rev().find(|o| o.version_id.is_none())
        } else {
            self.versions
                .iter()
                .rev()
                .find(|o| o.version_id.as_deref() == Some(version_id))
        }
    }

    pub fn find_mut(&mut self, version_id: &str) -> Option<&mut S3Object> {
        if version_id == "null" {
            self.versions
                .iter_mut()
                .rev()
                .find(|o| o.version_id.is_none())
        } else {
            self.versions
                .iter_mut()
                .rev()
                .find(|o| o.version_id.as_deref() == Some(version_id))
        }
    }

    /// Permanently remove a single version by ID, returning it if found.
    pub fn remove(&mut self, version_id: &str) -> Option<S3Object> {
        let target = if version_id == "null" {
            self.versions.iter().rposition(|o| o.version_id.is_none())
        } else {
            self.versions
                .iter()
                .rposition(|o| o.version_id.as_deref() == Some(version_id))
        };
        target.map(|idx| self.versions.remove(idx))
    }

    /// Append a new version at the top of the stack.
    pub fn push(&mut self, obj: S3Object) {
        self.versions.push(obj);
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, S3Object> {
        self.versions.iter()
    }
}

/// A multipart upload in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub key: String,
    /// Parts keyed by part number (1-based).
    pub parts: BTreeMap<u32, PartData>,
    pub created_at: String,
    pub bucket: String,
    pub content_type: String,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Data for a single uploaded part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartData {
    pub body: Body,
    pub etag: String,
}

/// Serializable snapshot of a single bucket (without object data bytes).
#[derive(Debug, Serialize, Deserialize)]
pub struct BucketSnapshot {
    pub name: String,
    pub region: String,
    pub created_at: String,
    pub versioning: VersioningStatus,
    pub tags: HashMap<String, String>,
    pub policy: Option<String>,
    pub cors: Option<String>,
    #[serde(default)]
    pub notification_config: NotificationConfiguration,
    #[serde(default)]
    pub acl: Option<String>,
    #[serde(default)]
    pub lifecycle: Option<String>,
    #[serde(default)]
    pub encryption: Option<String>,
    #[serde(default)]
    pub logging: Option<String>,
    #[serde(default)]
    pub configs: HashMap<String, String>,
    /// Object metadata only — `data` field is intentionally empty to avoid huge snapshots.
    pub objects: Vec<S3ObjectMetadata>,
}

/// Object metadata without the raw data bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectMetadata {
    pub key: String,
    pub content_type: String,
    pub content_length: u64,
    pub etag: String,
    pub last_modified: String,
    pub metadata: HashMap<String, String>,
    pub version_id: Option<String>,
    #[serde(default)]
    pub is_delete_marker: bool,
}

impl From<&S3Object> for S3ObjectMetadata {
    fn from(obj: &S3Object) -> Self {
        Self {
            key: obj.key.clone(),
            content_type: obj.content_type.clone(),
            content_length: obj.content_length,
            etag: obj.etag.clone(),
            last_modified: obj.last_modified.clone(),
            metadata: obj.metadata.clone(),
            version_id: obj.version_id.clone(),
            is_delete_marker: obj.is_delete_marker,
        }
    }
}

/// Serializable snapshot of `S3State`.
#[derive(Debug, Serialize, Deserialize)]
pub struct S3StateSnapshot {
    pub buckets: Vec<BucketSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct S3RegionSnapshot {
    pub account_id: String,
    pub region: String,
    pub buckets: Vec<BucketSnapshot>,
}

/// Global S3 state — all buckets are stored here.
#[derive(Debug, Default)]
pub struct S3State {
    /// Buckets keyed by bucket name.
    pub buckets: DashMap<String, Bucket>,
    /// Optional disk-backed body store, set once at service construction.
    pub body_store: OnceLock<Arc<BodyStore>>,
}

impl S3State {
    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.get()
    }

    pub fn set_body_store(&self, store: Arc<BodyStore>) {
        let _ = self.body_store.set(store);
    }
}

impl Snapshottable for S3State {
    type Snapshot = S3RegionSnapshot;

    fn to_snapshot(&self, account_id: &str, region: &str) -> Self::Snapshot {
        let buckets = self
            .buckets
            .iter()
            .map(|entry| {
                let b = entry.value();
                BucketSnapshot {
                    name: b.name.clone(),
                    region: b.region.clone(),
                    created_at: b.created_at.clone(),
                    versioning: b.versioning.clone(),
                    tags: b.tags.clone(),
                    policy: b.policy.clone(),
                    cors: b.cors.clone(),
                    notification_config: b.notification_config.clone(),
                    acl: b.acl.clone(),
                    lifecycle: b.lifecycle.clone(),
                    encryption: b.encryption.clone(),
                    logging: b.logging.clone(),
                    configs: b.configs.clone(),
                    objects: b
                        .objects
                        .iter()
                        .flat_map(|oe| {
                            oe.value()
                                .iter()
                                .map(S3ObjectMetadata::from)
                                .collect::<Vec<_>>()
                        })
                        .collect(),
                }
            })
            .collect();

        S3RegionSnapshot {
            account_id: account_id.to_string(),
            region: region.to_string(),
            buckets,
        }
    }

    fn from_snapshot(snapshot: Self::Snapshot) -> (String, String, Self) {
        let state = S3State::default();
        for bs in snapshot.buckets {
            let bucket = Bucket {
                name: bs.name.clone(),
                region: bs.region.clone(),
                created_at: bs.created_at.clone(),
                versioning: bs.versioning,
                tags: bs.tags,
                policy: bs.policy,
                cors: bs.cors,
                notification_config: bs.notification_config,
                acl: bs.acl,
                lifecycle: bs.lifecycle,
                encryption: bs.encryption,
                logging: bs.logging,
                configs: bs.configs,
                objects: {
                    let dm: DashMap<String, ObjectVersions> = DashMap::new();
                    for meta in bs.objects {
                        let obj = S3Object {
                            key: meta.key.clone(),
                            body: Body::InMemory(Vec::new()),
                            content_type: meta.content_type,
                            content_length: meta.content_length,
                            etag: meta.etag,
                            last_modified: meta.last_modified,
                            metadata: meta.metadata,
                            version_id: meta.version_id,
                            tags: Default::default(),
                            content_encoding: None,
                            cache_control: None,
                            content_disposition: None,
                            content_language: None,
                            expires: None,
                            is_delete_marker: meta.is_delete_marker,
                        };
                        dm.entry(meta.key).or_default().push(obj);
                    }
                    dm
                },
                multipart_uploads: DashMap::new(),
            };
            state.buckets.insert(bs.name, bucket);
        }
        (snapshot.account_id, snapshot.region, state)
    }
}
