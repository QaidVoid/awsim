use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Versioning status for a bucket.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VersioningStatus {
    #[default]
    Disabled,
    Enabled,
    Suspended,
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
    /// Objects keyed by object key.
    pub objects: DashMap<String, S3Object>,
    /// Multipart uploads keyed by upload ID.
    pub multipart_uploads: DashMap<String, MultipartUpload>,
}

impl Bucket {
    pub fn new(name: impl Into<String>, region: impl Into<String>, created_at: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            region: region.into(),
            created_at: created_at.into(),
            versioning: VersioningStatus::Disabled,
            tags: HashMap::new(),
            policy: None,
            cors: None,
            objects: DashMap::new(),
            multipart_uploads: DashMap::new(),
        }
    }
}

/// An S3 object stored in a bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Object {
    pub key: String,
    /// Raw object content.
    pub data: Vec<u8>,
    pub content_type: String,
    pub content_length: u64,
    /// MD5 hex digest wrapped in quotes, e.g. `"d41d8cd98f00b204e9800998ecf8427e"`.
    pub etag: String,
    /// RFC 7231 date string.
    pub last_modified: String,
    /// User-defined metadata from x-amz-meta-* headers.
    pub metadata: HashMap<String, String>,
    pub version_id: Option<String>,
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
}

/// Data for a single uploaded part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartData {
    pub data: Vec<u8>,
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
        }
    }
}

/// Serializable snapshot of `S3State`.
#[derive(Debug, Serialize, Deserialize)]
pub struct S3StateSnapshot {
    pub buckets: Vec<BucketSnapshot>,
}

/// Global S3 state — all buckets are stored here.
#[derive(Debug, Default)]
pub struct S3State {
    /// Buckets keyed by bucket name.
    pub buckets: DashMap<String, Bucket>,
}
