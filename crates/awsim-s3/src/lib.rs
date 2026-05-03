#![deny(warnings)]

pub mod authz;
mod operations;
pub mod state;
mod util;

pub use authz::S3ResourcePolicyLookup;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, BlobInventory, BodyStore, InternalEvent, Protocol,
    RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::{S3State, S3StateSnapshot};

/// Check whether an event name matches any of the configured event filters.
/// Supports wildcard suffixes, e.g. "s3:ObjectCreated:*" matches any ObjectCreated event.
fn event_matches(filters: &[String], event_name: &str) -> bool {
    for filter in filters {
        if filter == event_name {
            return true;
        }
        if let Some(prefix) = filter.strip_suffix('*')
            && event_name.starts_with(prefix)
        {
            return true;
        }
    }
    false
}

/// The AWSim S3 service handler.
pub struct S3Service {
    store: AccountRegionStore<S3State>,
    body_store: Option<Arc<BodyStore>>,
}

impl S3Service {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            body_store: None,
        }
    }

    pub fn with_data_dir(dir: impl AsRef<std::path::Path>) -> Self {
        let root = dir.as_ref().join("s3");
        Self {
            store: AccountRegionStore::new(),
            body_store: Some(Arc::new(BodyStore::new(root))),
        }
    }

    pub fn with_max_blob_bytes(mut self, bytes: u64) -> Self {
        if let Some(bs) = self.body_store.take() {
            let root = bs.root().to_path_buf();
            self.body_store = Some(Arc::new(BodyStore::new(root).with_max_size(bytes)));
        }
        self
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<S3State> {
        let state = self.store.get(&ctx.account_id, "global");
        if let Some(bs) = &self.body_store {
            state.set_body_store(Arc::clone(bs));
        }
        state
    }

    pub fn store(&self) -> AccountRegionStore<S3State> {
        self.store.clone()
    }

    pub fn body_store(&self) -> Option<&Arc<BodyStore>> {
        self.body_store.as_ref()
    }

    pub const GROUPS: &'static [&'static str] = &["objects", "multipart"];

    /// Bulk-seed `buckets` empty buckets, each with `objects_per_bucket`
    /// small text objects. Object body is a deterministic in-memory blob
    /// of `body_bytes` length so the seed is fast + repeatable. Skips
    /// SigV4 / gateway entirely — a 100-bucket × 100-object seed lands
    /// in well under a second.
    pub fn seed(&self, input: SeedDatasetInput) -> SeedDatasetOutput {
        use awsim_core::Body;
        use state::{Bucket, ObjectVersions, S3Object};
        use uuid::Uuid;
        let region_label = "global";
        let state = self.store.get(&input.account, region_label);
        let now = chrono::Utc::now().to_rfc3339();
        let body_bytes = input.body_bytes.min(64 * 1024) as usize;
        let body_data: Vec<u8> = (0..body_bytes).map(|i| b'a' + (i as u8 % 26)).collect();
        // Stable etag for the shared body — md5 of the byte string.
        let etag_hex = md5_hex(&body_data);

        let mut buckets_created = 0u64;
        let mut objects_created = 0u64;

        for b in 0..input.buckets {
            let name = format!("{}-{}-{}", input.prefix, b, Uuid::new_v4().simple());
            let bucket = Bucket::new(&name, &input.region, &now);
            for o in 0..input.objects_per_bucket {
                let key = format!("seed/{o}.txt");
                let object = S3Object {
                    key: key.clone(),
                    body: Body::InMemory(body_data.clone()),
                    content_type: "text/plain".to_string(),
                    content_length: body_data.len() as u64,
                    etag: format!("\"{etag_hex}\""),
                    last_modified: now.clone(),
                    metadata: std::collections::HashMap::new(),
                    version_id: None,
                    tags: std::collections::HashMap::new(),
                    is_delete_marker: false,
                };
                bucket.objects.insert(
                    key,
                    ObjectVersions {
                        versions: vec![object],
                    },
                );
                objects_created += 1;
            }
            state.buckets.insert(name, bucket);
            buckets_created += 1;
        }

        SeedDatasetOutput {
            buckets_created,
            objects_created,
        }
    }
}

/// Input shape for `S3Service::seed`.
pub struct SeedDatasetInput {
    pub account: String,
    pub region: String,
    pub buckets: u64,
    pub objects_per_bucket: u64,
    /// Body bytes per object — capped at 64 KiB by the seeder.
    pub body_bytes: u64,
    pub prefix: String,
}

/// Result shape returned by `S3Service::seed`.
pub struct SeedDatasetOutput {
    pub buckets_created: u64,
    pub objects_created: u64,
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(32);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

impl Default for S3Service {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobInventory for S3Service {
    fn known_blobs(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (_, state) in self.store.iter_all() {
            for bucket_entry in state.buckets.iter() {
                let bucket_name = bucket_entry.key().clone();
                let bucket = bucket_entry.value();
                for obj_entry in bucket.objects.iter() {
                    let key = obj_entry.key().clone();
                    for version in obj_entry.value().iter() {
                        if version.is_delete_marker {
                            continue;
                        }
                        let blob_key = operations::object::versioned_blob_key(
                            &key,
                            version.version_id.as_deref(),
                        );
                        out.push(("objects".to_string(), bucket_name.clone(), blob_key));
                    }
                }
                for mp_entry in bucket.multipart_uploads.iter() {
                    let upload_id = mp_entry.key().clone();
                    for part_number in mp_entry.value().parts.keys() {
                        out.push((
                            "multipart".to_string(),
                            bucket_name.clone(),
                            format!("{upload_id}/{part_number}"),
                        ));
                    }
                }
            }
        }
        out
    }
}

#[async_trait]
impl ServiceHandler for S3Service {
    fn service_name(&self) -> &str {
        "s3"
    }

    fn signing_name(&self) -> &str {
        "s3"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestXml
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // ── Bucket-level operations ──────────────────────────────────────
            // GET / — list all buckets
            RouteDefinition {
                method: "GET",
                path_pattern: "/",
                operation: "ListBuckets",
                required_query_param: None,
            },
            // HEAD /{Bucket}
            RouteDefinition {
                method: "HEAD",
                path_pattern: "/{Bucket}",
                operation: "HeadBucket",
                required_query_param: None,
            },
            // PUT /{Bucket}?versioning
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketVersioning",
                required_query_param: Some("versioning"),
            },
            // GET /{Bucket}?versioning
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketVersioning",
                required_query_param: Some("versioning"),
            },
            // PUT /{Bucket}?tagging
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketTagging",
                required_query_param: Some("tagging"),
            },
            // GET /{Bucket}?tagging
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketTagging",
                required_query_param: Some("tagging"),
            },
            // DELETE /{Bucket}?tagging
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketTagging",
                required_query_param: Some("tagging"),
            },
            // PUT /{Bucket}?policy
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketPolicy",
                required_query_param: Some("policy"),
            },
            // GET /{Bucket}?policy
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketPolicy",
                required_query_param: Some("policy"),
            },
            // DELETE /{Bucket}?policy
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketPolicy",
                required_query_param: Some("policy"),
            },
            // PUT /{Bucket}?cors
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketCors",
                required_query_param: Some("cors"),
            },
            // GET /{Bucket}?cors
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketCors",
                required_query_param: Some("cors"),
            },
            // DELETE /{Bucket}?cors
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketCors",
                required_query_param: Some("cors"),
            },
            // GET /{Bucket}?location
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketLocation",
                required_query_param: Some("location"),
            },
            // PUT /{Bucket}?notification
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketNotificationConfiguration",
                required_query_param: Some("notification"),
            },
            // GET /{Bucket}?notification
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketNotificationConfiguration",
                required_query_param: Some("notification"),
            },
            // GET /{Bucket}?acl
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketAcl",
                required_query_param: Some("acl"),
            },
            // PUT /{Bucket}?acl
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketAcl",
                required_query_param: Some("acl"),
            },
            // GET /{Bucket}?lifecycle
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketLifecycleConfiguration",
                required_query_param: Some("lifecycle"),
            },
            // PUT /{Bucket}?lifecycle
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketLifecycleConfiguration",
                required_query_param: Some("lifecycle"),
            },
            // DELETE /{Bucket}?lifecycle
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketLifecycleConfiguration",
                required_query_param: Some("lifecycle"),
            },
            // GET /{Bucket}?encryption
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketEncryption",
                required_query_param: Some("encryption"),
            },
            // PUT /{Bucket}?encryption
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketEncryption",
                required_query_param: Some("encryption"),
            },
            // DELETE /{Bucket}?encryption
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketEncryption",
                required_query_param: Some("encryption"),
            },
            // GET /{Bucket}?logging
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketLogging",
                required_query_param: Some("logging"),
            },
            // PUT /{Bucket}?logging
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketLogging",
                required_query_param: Some("logging"),
            },
            // GET /{Bucket}?website
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketWebsite",
                required_query_param: Some("website"),
            },
            // PUT /{Bucket}?website
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketWebsite",
                required_query_param: Some("website"),
            },
            // DELETE /{Bucket}?website
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketWebsite",
                required_query_param: Some("website"),
            },
            // GET /{Bucket}?replication
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketReplication",
                required_query_param: Some("replication"),
            },
            // PUT /{Bucket}?replication
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketReplication",
                required_query_param: Some("replication"),
            },
            // DELETE /{Bucket}?replication
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketReplication",
                required_query_param: Some("replication"),
            },
            // GET /{Bucket}?requestPayment
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketRequestPayment",
                required_query_param: Some("requestPayment"),
            },
            // PUT /{Bucket}?requestPayment
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketRequestPayment",
                required_query_param: Some("requestPayment"),
            },
            // GET /{Bucket}?accelerate
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketAccelerateConfiguration",
                required_query_param: Some("accelerate"),
            },
            // PUT /{Bucket}?accelerate
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketAccelerateConfiguration",
                required_query_param: Some("accelerate"),
            },
            // GET /{Bucket}?analytics (with Id query param handled in handler)
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketAnalyticsConfiguration",
                required_query_param: Some("analytics"),
            },
            // PUT /{Bucket}?analytics
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketAnalyticsConfiguration",
                required_query_param: Some("analytics"),
            },
            // DELETE /{Bucket}?analytics
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketAnalyticsConfiguration",
                required_query_param: Some("analytics"),
            },
            // GET /{Bucket}?metrics
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketMetricsConfiguration",
                required_query_param: Some("metrics"),
            },
            // PUT /{Bucket}?metrics
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketMetricsConfiguration",
                required_query_param: Some("metrics"),
            },
            // DELETE /{Bucket}?metrics
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketMetricsConfiguration",
                required_query_param: Some("metrics"),
            },
            // GET /{Bucket}?intelligent-tiering
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketIntelligentTieringConfiguration",
                required_query_param: Some("intelligent-tiering"),
            },
            // PUT /{Bucket}?intelligent-tiering
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketIntelligentTieringConfiguration",
                required_query_param: Some("intelligent-tiering"),
            },
            // DELETE /{Bucket}?intelligent-tiering
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketIntelligentTieringConfiguration",
                required_query_param: Some("intelligent-tiering"),
            },
            // GET /{Bucket}?inventory
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketInventoryConfiguration",
                required_query_param: Some("inventory"),
            },
            // PUT /{Bucket}?inventory
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketInventoryConfiguration",
                required_query_param: Some("inventory"),
            },
            // DELETE /{Bucket}?inventory
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketInventoryConfiguration",
                required_query_param: Some("inventory"),
            },
            // GET /{Bucket}?ownershipControls
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketOwnershipControls",
                required_query_param: Some("ownershipControls"),
            },
            // PUT /{Bucket}?ownershipControls
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutBucketOwnershipControls",
                required_query_param: Some("ownershipControls"),
            },
            // DELETE /{Bucket}?ownershipControls
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucketOwnershipControls",
                required_query_param: Some("ownershipControls"),
            },
            // GET /{Bucket}?publicAccessBlock
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetPublicAccessBlock",
                required_query_param: Some("publicAccessBlock"),
            },
            // PUT /{Bucket}?publicAccessBlock
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutPublicAccessBlock",
                required_query_param: Some("publicAccessBlock"),
            },
            // DELETE /{Bucket}?publicAccessBlock
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeletePublicAccessBlock",
                required_query_param: Some("publicAccessBlock"),
            },
            // GET /{Bucket}?list-type=2
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "ListObjectsV2",
                required_query_param: Some("list-type"),
            },
            // GET /{Bucket}?versions
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "ListObjectVersions",
                required_query_param: Some("versions"),
            },
            // GET /{Bucket}?policyStatus
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetBucketPolicyStatus",
                required_query_param: Some("policyStatus"),
            },
            // GET /{Bucket}?object-lock
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "GetObjectLockConfiguration",
                required_query_param: Some("object-lock"),
            },
            // PUT /{Bucket}?object-lock
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "PutObjectLockConfiguration",
                required_query_param: Some("object-lock"),
            },
            // GET /{Bucket}?session
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "CreateSession",
                required_query_param: Some("session"),
            },
            // GET /{Bucket}?uploads  (list multipart uploads)
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "ListMultipartUploads",
                required_query_param: Some("uploads"),
            },
            // POST /{Bucket}?delete
            RouteDefinition {
                method: "POST",
                path_pattern: "/{Bucket}",
                operation: "DeleteObjects",
                required_query_param: Some("delete"),
            },
            // POST /{Bucket}/{Key+}?select — SelectObjectContent stub
            RouteDefinition {
                method: "POST",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "SelectObjectContent",
                required_query_param: Some("select"),
            },
            // POST /{Bucket}/{Key+}?restore
            RouteDefinition {
                method: "POST",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "RestoreObject",
                required_query_param: Some("restore"),
            },
            // GET /{Bucket}/{Key+}?legal-hold
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObjectLegalHold",
                required_query_param: Some("legal-hold"),
            },
            // PUT /{Bucket}/{Key+}?legal-hold
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "PutObjectLegalHold",
                required_query_param: Some("legal-hold"),
            },
            // GET /{Bucket}/{Key+}?retention
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObjectRetention",
                required_query_param: Some("retention"),
            },
            // PUT /{Bucket}/{Key+}?retention
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "PutObjectRetention",
                required_query_param: Some("retention"),
            },
            // GET /{Bucket}/{Key+}?attributes
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObjectAttributes",
                required_query_param: Some("attributes"),
            },
            // PUT /{Bucket}/{Key+}?acl
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "PutObjectAcl",
                required_query_param: Some("acl"),
            },
            // PUT /{Bucket}/{Key+}?renameObject
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "RenameObject",
                required_query_param: Some("renameObject"),
            },
            // GET /{Bucket} — list objects v1 (no query param; must come after all specific ones)
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "ListObjects",
                required_query_param: None,
            },
            // PUT /{Bucket} — create bucket (no query param; must come after all specific ones)
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}",
                operation: "CreateBucket",
                required_query_param: None,
            },
            // DELETE /{Bucket}
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}",
                operation: "DeleteBucket",
                required_query_param: None,
            },
            // ── Object-level operations ──────────────────────────────────────
            // PUT /{Bucket}/{Key+}?partNumber=...  — upload part
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "UploadPart",
                required_query_param: Some("partNumber"),
            },
            // POST /{Bucket}/{Key+}?uploads  — initiate multipart upload
            RouteDefinition {
                method: "POST",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "CreateMultipartUpload",
                required_query_param: Some("uploads"),
            },
            // POST /{Bucket}/{Key+}?uploadId=...  — complete multipart upload
            RouteDefinition {
                method: "POST",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "CompleteMultipartUpload",
                required_query_param: Some("uploadId"),
            },
            // DELETE /{Bucket}/{Key+}?uploadId=...  — abort multipart upload
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "AbortMultipartUpload",
                required_query_param: Some("uploadId"),
            },
            // GET /{Bucket}/{Key+}?uploadId=...  — list parts
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "ListParts",
                required_query_param: Some("uploadId"),
            },
            // PUT /{Bucket}/{Key+}?tagging — put object tagging
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "PutObjectTagging",
                required_query_param: Some("tagging"),
            },
            // GET /{Bucket}/{Key+}?tagging — get object tagging
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObjectTagging",
                required_query_param: Some("tagging"),
            },
            // DELETE /{Bucket}/{Key+}?tagging — delete object tagging
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "DeleteObjectTagging",
                required_query_param: Some("tagging"),
            },
            // GET /{Bucket}/{Key+}?acl — get object ACL
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObjectAcl",
                required_query_param: Some("acl"),
            },
            // PUT /{Bucket}/{Key+}  — put object (or copy object via header)
            RouteDefinition {
                method: "PUT",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "PutObject",
                required_query_param: None,
            },
            // GET /{Bucket}/{Key+}
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "GetObject",
                required_query_param: None,
            },
            // HEAD /{Bucket}/{Key+}
            RouteDefinition {
                method: "HEAD",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "HeadObject",
                required_query_param: None,
            },
            // DELETE /{Bucket}/{Key+}
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/{Bucket}/{Key+}",
                operation: "DeleteObject",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "S3 request");
        let state = self.get_state(ctx);

        match operation {
            // Bucket operations
            "ListBuckets" => operations::bucket::list_buckets(&state, ctx),
            "CreateBucket" => operations::bucket::create_bucket(&state, &input, ctx),
            "DeleteBucket" => operations::bucket::delete_bucket(&state, &input),
            "HeadBucket" => operations::bucket::head_bucket(&state, &input),
            "GetBucketLocation" => operations::bucket::get_bucket_location(&state, &input),

            // Configuration
            "PutBucketTagging" => operations::config::put_bucket_tagging(&state, &input),
            "GetBucketTagging" => operations::config::get_bucket_tagging(&state, &input),
            "DeleteBucketTagging" => operations::config::delete_bucket_tagging(&state, &input),
            "PutObjectTagging" => operations::config::put_object_tagging(&state, &input),
            "GetObjectTagging" => operations::config::get_object_tagging(&state, &input),
            "DeleteObjectTagging" => operations::config::delete_object_tagging(&state, &input),
            "PutBucketVersioning" => operations::config::put_bucket_versioning(&state, &input),
            "GetBucketVersioning" => operations::config::get_bucket_versioning(&state, &input),
            "PutBucketPolicy" => operations::config::put_bucket_policy(&state, &input),
            "GetBucketPolicy" => operations::config::get_bucket_policy(&state, &input),
            "DeleteBucketPolicy" => operations::config::delete_bucket_policy(&state, &input),
            "PutBucketCors" => operations::config::put_bucket_cors(&state, &input),
            "GetBucketCors" => operations::config::get_bucket_cors(&state, &input),
            "DeleteBucketCors" => operations::config::delete_bucket_cors(&state, &input),
            "PutBucketNotificationConfiguration" => {
                operations::config::put_bucket_notification_configuration(&state, &input)
            }
            "GetBucketNotificationConfiguration" => {
                operations::config::get_bucket_notification_configuration(&state, &input)
            }
            "GetBucketAcl" => operations::config::get_bucket_acl(&state, &input, ctx),
            "PutBucketAcl" => operations::config::put_bucket_acl(&state, &input),
            "GetObjectAcl" => operations::config::get_object_acl(&state, &input, ctx),
            "GetBucketLifecycleConfiguration" => {
                operations::config::get_bucket_lifecycle_configuration(&state, &input)
            }
            "PutBucketLifecycleConfiguration" => {
                operations::config::put_bucket_lifecycle_configuration(&state, &input)
            }
            "DeleteBucketLifecycleConfiguration" => {
                operations::config::delete_bucket_lifecycle_configuration(&state, &input)
            }
            "GetBucketEncryption" => operations::config::get_bucket_encryption(&state, &input),
            "PutBucketEncryption" => operations::config::put_bucket_encryption(&state, &input),
            "DeleteBucketEncryption" => {
                operations::config::delete_bucket_encryption(&state, &input)
            }
            "GetBucketLogging" => operations::config::get_bucket_logging(&state, &input),
            "PutBucketLogging" => operations::config::put_bucket_logging(&state, &input),

            // Website
            "GetBucketWebsite" => operations::config::get_bucket_website(&state, &input),
            "PutBucketWebsite" => operations::config::put_bucket_website(&state, &input),
            "DeleteBucketWebsite" => operations::config::delete_bucket_website(&state, &input),

            // Replication
            "GetBucketReplication" => operations::config::get_bucket_replication(&state, &input),
            "PutBucketReplication" => operations::config::put_bucket_replication(&state, &input),
            "DeleteBucketReplication" => {
                operations::config::delete_bucket_replication(&state, &input)
            }

            // Request Payment
            "GetBucketRequestPayment" => {
                operations::config::get_bucket_request_payment(&state, &input)
            }
            "PutBucketRequestPayment" => {
                operations::config::put_bucket_request_payment(&state, &input)
            }

            // Accelerate
            "GetBucketAccelerateConfiguration" => {
                operations::config::get_bucket_accelerate_configuration(&state, &input)
            }
            "PutBucketAccelerateConfiguration" => {
                operations::config::put_bucket_accelerate_configuration(&state, &input)
            }

            // Analytics (Get handles List when Id is absent)
            "GetBucketAnalyticsConfiguration" | "ListBucketAnalyticsConfigurations" => {
                operations::config::get_bucket_analytics_configuration(&state, &input)
            }
            "PutBucketAnalyticsConfiguration" => {
                operations::config::put_bucket_analytics_configuration(&state, &input)
            }
            "DeleteBucketAnalyticsConfiguration" => {
                operations::config::delete_bucket_analytics_configuration(&state, &input)
            }

            // Metrics (Get handles List when Id is absent)
            "GetBucketMetricsConfiguration" | "ListBucketMetricsConfigurations" => {
                operations::config::get_bucket_metrics_configuration(&state, &input)
            }
            "PutBucketMetricsConfiguration" => {
                operations::config::put_bucket_metrics_configuration(&state, &input)
            }
            "DeleteBucketMetricsConfiguration" => {
                operations::config::delete_bucket_metrics_configuration(&state, &input)
            }

            // Intelligent Tiering (Get handles List when Id is absent)
            "GetBucketIntelligentTieringConfiguration"
            | "ListBucketIntelligentTieringConfigurations" => {
                operations::config::get_bucket_intelligent_tiering_configuration(&state, &input)
            }
            "PutBucketIntelligentTieringConfiguration" => {
                operations::config::put_bucket_intelligent_tiering_configuration(&state, &input)
            }
            "DeleteBucketIntelligentTieringConfiguration" => {
                operations::config::delete_bucket_intelligent_tiering_configuration(&state, &input)
            }

            // Inventory (Get handles List when Id is absent)
            "GetBucketInventoryConfiguration" | "ListBucketInventoryConfigurations" => {
                operations::config::get_bucket_inventory_configuration(&state, &input)
            }
            "PutBucketInventoryConfiguration" => {
                operations::config::put_bucket_inventory_configuration(&state, &input)
            }
            "DeleteBucketInventoryConfiguration" => {
                operations::config::delete_bucket_inventory_configuration(&state, &input)
            }

            // Ownership Controls
            "GetBucketOwnershipControls" => {
                operations::config::get_bucket_ownership_controls(&state, &input)
            }
            "PutBucketOwnershipControls" => {
                operations::config::put_bucket_ownership_controls(&state, &input)
            }
            "DeleteBucketOwnershipControls" => {
                operations::config::delete_bucket_ownership_controls(&state, &input)
            }

            // Public Access Block
            "GetPublicAccessBlock" => operations::config::get_public_access_block(&state, &input),
            "PutPublicAccessBlock" => operations::config::put_public_access_block(&state, &input),
            "DeletePublicAccessBlock" => {
                operations::config::delete_public_access_block(&state, &input)
            }

            // SelectObjectContent (stub)
            "SelectObjectContent" => operations::config::select_object_content(&state, &input),

            // Bucket Policy Status
            "GetBucketPolicyStatus" => operations::config::get_bucket_policy_status(&state, &input),

            // Object Lock
            "GetObjectLockConfiguration" => {
                operations::config::get_object_lock_configuration(&state, &input)
            }
            "PutObjectLockConfiguration" => {
                operations::config::put_object_lock_configuration(&state, &input)
            }

            // Object Legal Hold
            "GetObjectLegalHold" => operations::config::get_object_legal_hold(&state, &input),
            "PutObjectLegalHold" => operations::config::put_object_legal_hold(&state, &input),

            // Object Retention
            "GetObjectRetention" => operations::config::get_object_retention(&state, &input),
            "PutObjectRetention" => operations::config::put_object_retention(&state, &input),

            // Object Attributes
            "GetObjectAttributes" => operations::config::get_object_attributes(&state, &input),

            // Put Object ACL
            "PutObjectAcl" => operations::config::put_object_acl(&state, &input),

            // Restore Object
            "RestoreObject" => operations::config::restore_object(&state, &input),

            // Rename Object
            "RenameObject" => operations::config::rename_object(&state, &input),

            // Create Session
            "CreateSession" => operations::config::create_session(&state, &input),

            // Object operations
            "PutObject" => {
                let result = operations::object::put_object(&state, &input, ctx)?;
                // Emit s3:ObjectCreated:Put notification if configured
                if let Some(bus) = &ctx.event_bus {
                    let bucket_name = input.get("Bucket").and_then(Value::as_str).unwrap_or("");
                    let key = input.get("Key").and_then(Value::as_str).unwrap_or("");
                    if let Some(bucket) = state.buckets.get(bucket_name)
                        && !bucket.notification_config.destinations.is_empty()
                    {
                        let etag = result
                            .get("ETag")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let size = bucket
                            .objects
                            .get(key)
                            .and_then(|v| v.current().map(|o| o.content_length))
                            .unwrap_or(0);
                        let configured_destinations: Vec<serde_json::Value> = bucket
                            .notification_config
                            .destinations
                            .iter()
                            .filter(|d| event_matches(&d.events, "s3:ObjectCreated:Put"))
                            .map(|d| serde_json::json!({ "type": d.dest_type, "arn": d.arn }))
                            .collect();
                        if !configured_destinations.is_empty() {
                            bus.publish(InternalEvent {
                                source: "s3".to_string(),
                                event_type: "s3:ObjectCreated:Put".to_string(),
                                region: ctx.region.clone(),
                                account_id: ctx.account_id.clone(),
                                detail: serde_json::json!({
                                    "bucket": {
                                        "name": bucket_name,
                                        "arn": format!("arn:aws:s3:::{}", bucket_name),
                                    },
                                    "object": {
                                        "key": key,
                                        "size": size,
                                        "eTag": etag,
                                    },
                                    "configuredDestinations": configured_destinations,
                                }),
                            });
                        }
                    }
                }
                Ok(result)
            }
            "CopyObject" => {
                let result = operations::object::put_object(&state, &input, ctx)?;
                // Emit s3:ObjectCreated:Copy notification if configured
                if let Some(bus) = &ctx.event_bus {
                    let bucket_name = input.get("Bucket").and_then(Value::as_str).unwrap_or("");
                    let key = input.get("Key").and_then(Value::as_str).unwrap_or("");
                    if let Some(bucket) = state.buckets.get(bucket_name)
                        && !bucket.notification_config.destinations.is_empty()
                    {
                        let (size, etag) = bucket
                            .objects
                            .get(key)
                            .and_then(|v| v.current().map(|o| (o.content_length, o.etag.clone())))
                            .unwrap_or((0, String::new()));
                        let configured_destinations: Vec<serde_json::Value> = bucket
                            .notification_config
                            .destinations
                            .iter()
                            .filter(|d| event_matches(&d.events, "s3:ObjectCreated:Copy"))
                            .map(|d| serde_json::json!({ "type": d.dest_type, "arn": d.arn }))
                            .collect();
                        if !configured_destinations.is_empty() {
                            bus.publish(InternalEvent {
                                source: "s3".to_string(),
                                event_type: "s3:ObjectCreated:Copy".to_string(),
                                region: ctx.region.clone(),
                                account_id: ctx.account_id.clone(),
                                detail: serde_json::json!({
                                    "bucket": {
                                        "name": bucket_name,
                                        "arn": format!("arn:aws:s3:::{}", bucket_name),
                                    },
                                    "object": {
                                        "key": key,
                                        "size": size,
                                        "eTag": etag,
                                    },
                                    "configuredDestinations": configured_destinations,
                                }),
                            });
                        }
                    }
                }
                Ok(result)
            }
            "DeleteObject" => {
                // Capture info before deletion for the event
                let bucket_name = input
                    .get("Bucket")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let key = input
                    .get("Key")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let configured_destinations: Vec<serde_json::Value> =
                    if let Some(bucket) = state.buckets.get(&bucket_name) {
                        bucket
                            .notification_config
                            .destinations
                            .iter()
                            .filter(|d| event_matches(&d.events, "s3:ObjectRemoved:Delete"))
                            .map(|d| serde_json::json!({ "type": d.dest_type, "arn": d.arn }))
                            .collect()
                    } else {
                        Vec::new()
                    };

                let result = operations::object::delete_object(&state, &input)?;

                // Emit s3:ObjectRemoved:Delete notification if configured
                if let Some(bus) = &ctx.event_bus
                    && !configured_destinations.is_empty()
                {
                    bus.publish(InternalEvent {
                        source: "s3".to_string(),
                        event_type: "s3:ObjectRemoved:Delete".to_string(),
                        region: ctx.region.clone(),
                        account_id: ctx.account_id.clone(),
                        detail: serde_json::json!({
                            "bucket": {
                                "name": bucket_name,
                                "arn": format!("arn:aws:s3:::{}", bucket_name),
                            },
                            "object": {
                                "key": key,
                            },
                            "configuredDestinations": configured_destinations,
                        }),
                    });
                }
                Ok(result)
            }
            "GetObject" => operations::object::get_object(&state, &input, ctx),
            "HeadObject" => operations::object::head_object(&state, &input),

            // Listing / batch
            "ListObjectsV2" => operations::list::list_objects_v2(&state, &input, ctx),
            "ListObjects" => operations::list::list_objects(&state, &input, ctx),
            "ListObjectVersions" => operations::list::list_object_versions(&state, &input),
            "DeleteObjects" => operations::list::delete_objects(&state, &input),

            // Multipart
            "CreateMultipartUpload" => {
                operations::multipart::create_multipart_upload(&state, &input)
            }
            "UploadPart" => {
                // x-amz-copy-source disambiguates UploadPart vs UploadPartCopy
                // (both are PUT /{Bucket}/{Key+}?partNumber=&uploadId=).
                if input.get("CopySource").is_some() {
                    operations::multipart::upload_part_copy(&state, &input)
                } else {
                    operations::multipart::upload_part(&state, &input)
                }
            }
            "UploadPartCopy" => operations::multipart::upload_part_copy(&state, &input),
            "CompleteMultipartUpload" => {
                operations::multipart::complete_multipart_upload(&state, &input)
            }
            "AbortMultipartUpload" => operations::multipart::abort_multipart_upload(&state, &input),
            "ListMultipartUploads" => operations::multipart::list_multipart_uploads(&state, &input),
            "ListParts" => operations::multipart::list_parts(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.store.snapshot_to_bytes()
    }

    fn iam_action(&self, operation: &str) -> Option<String> {
        match operation {
            "ListBuckets"
            | "CreateBucket"
            | "DeleteBucket"
            | "HeadBucket"
            | "GetBucketLocation"
            | "PutBucketTagging"
            | "GetBucketTagging"
            | "DeleteBucketTagging"
            | "PutObjectTagging"
            | "GetObjectTagging"
            | "DeleteObjectTagging"
            | "PutBucketVersioning"
            | "GetBucketVersioning"
            | "PutBucketPolicy"
            | "GetBucketPolicy"
            | "DeleteBucketPolicy"
            | "PutBucketCors"
            | "GetBucketCors"
            | "DeleteBucketCors"
            | "PutBucketNotificationConfiguration"
            | "GetBucketNotificationConfiguration"
            | "GetBucketAcl"
            | "PutBucketAcl"
            | "GetObjectAcl"
            | "PutObjectAcl"
            | "GetBucketLifecycleConfiguration"
            | "PutBucketLifecycleConfiguration"
            | "DeleteBucketLifecycleConfiguration"
            | "GetEncryptionConfiguration"
            | "PutEncryptionConfiguration"
            | "GetBucketEncryption"
            | "PutBucketEncryption"
            | "DeleteBucketEncryption"
            | "GetBucketLogging"
            | "PutBucketLogging"
            | "GetBucketWebsite"
            | "PutBucketWebsite"
            | "DeleteBucketWebsite"
            | "GetBucketReplication"
            | "PutBucketReplication"
            | "DeleteBucketReplication"
            | "GetBucketRequestPayment"
            | "PutBucketRequestPayment"
            | "GetBucketAccelerateConfiguration"
            | "PutBucketAccelerateConfiguration"
            | "GetBucketAnalyticsConfiguration"
            | "PutBucketAnalyticsConfiguration"
            | "DeleteBucketAnalyticsConfiguration"
            | "ListBucketAnalyticsConfigurations"
            | "GetBucketMetricsConfiguration"
            | "PutBucketMetricsConfiguration"
            | "DeleteBucketMetricsConfiguration"
            | "ListBucketMetricsConfigurations"
            | "GetBucketIntelligentTieringConfiguration"
            | "PutBucketIntelligentTieringConfiguration"
            | "DeleteBucketIntelligentTieringConfiguration"
            | "ListBucketIntelligentTieringConfigurations"
            | "GetBucketInventoryConfiguration"
            | "PutBucketInventoryConfiguration"
            | "DeleteBucketInventoryConfiguration"
            | "ListBucketInventoryConfigurations"
            | "GetBucketOwnershipControls"
            | "PutBucketOwnershipControls"
            | "DeleteBucketOwnershipControls"
            | "GetPublicAccessBlock"
            | "PutPublicAccessBlock"
            | "DeletePublicAccessBlock"
            | "GetBucketPolicyStatus"
            | "GetObjectLockConfiguration"
            | "PutObjectLockConfiguration"
            | "GetObjectLegalHold"
            | "PutObjectLegalHold"
            | "GetObjectRetention"
            | "PutObjectRetention"
            | "GetObjectAttributes"
            | "RestoreObject"
            | "RenameObject"
            | "CreateSession"
            | "PutObject"
            | "GetObject"
            | "HeadObject"
            | "DeleteObject"
            | "CopyObject"
            | "ListObjects"
            | "ListObjectsV2"
            | "ListObjectVersions"
            | "ListBucket"
            | "DeleteObjects"
            | "CreateMultipartUpload"
            | "UploadPart"
            | "UploadPartCopy"
            | "CompleteMultipartUpload"
            | "AbortMultipartUpload"
            | "ListMultipartUploads"
            | "ListParts"
            | "SelectObjectContent" => Some(format!("s3:{operation}")),
            _ => None,
        }
    }

    fn iam_resource(
        &self,
        operation: &str,
        input: &Value,
        _ctx: &RequestContext,
    ) -> Option<String> {
        match operation {
            "ListBuckets" => Some("*".to_string()),
            "ListObjects"
            | "ListObjectsV2"
            | "ListObjectVersions"
            | "ListBucket"
            | "CreateBucket"
            | "DeleteBucket"
            | "HeadBucket"
            | "GetBucketLocation"
            | "PutBucketTagging"
            | "GetBucketTagging"
            | "DeleteBucketTagging"
            | "PutBucketVersioning"
            | "GetBucketVersioning"
            | "PutBucketPolicy"
            | "GetBucketPolicy"
            | "DeleteBucketPolicy"
            | "PutBucketCors"
            | "GetBucketCors"
            | "DeleteBucketCors"
            | "PutBucketNotificationConfiguration"
            | "GetBucketNotificationConfiguration"
            | "GetBucketAcl"
            | "PutBucketAcl"
            | "GetBucketLifecycleConfiguration"
            | "PutBucketLifecycleConfiguration"
            | "DeleteBucketLifecycleConfiguration"
            | "GetBucketEncryption"
            | "PutBucketEncryption"
            | "DeleteBucketEncryption"
            | "GetBucketLogging"
            | "PutBucketLogging"
            | "GetBucketWebsite"
            | "PutBucketWebsite"
            | "DeleteBucketWebsite"
            | "GetBucketReplication"
            | "PutBucketReplication"
            | "DeleteBucketReplication"
            | "GetBucketRequestPayment"
            | "PutBucketRequestPayment"
            | "GetBucketAccelerateConfiguration"
            | "PutBucketAccelerateConfiguration"
            | "GetBucketAnalyticsConfiguration"
            | "PutBucketAnalyticsConfiguration"
            | "DeleteBucketAnalyticsConfiguration"
            | "ListBucketAnalyticsConfigurations"
            | "GetBucketMetricsConfiguration"
            | "PutBucketMetricsConfiguration"
            | "DeleteBucketMetricsConfiguration"
            | "ListBucketMetricsConfigurations"
            | "GetBucketIntelligentTieringConfiguration"
            | "PutBucketIntelligentTieringConfiguration"
            | "DeleteBucketIntelligentTieringConfiguration"
            | "ListBucketIntelligentTieringConfigurations"
            | "GetBucketInventoryConfiguration"
            | "PutBucketInventoryConfiguration"
            | "DeleteBucketInventoryConfiguration"
            | "ListBucketInventoryConfigurations"
            | "GetBucketOwnershipControls"
            | "PutBucketOwnershipControls"
            | "DeleteBucketOwnershipControls"
            | "GetPublicAccessBlock"
            | "PutPublicAccessBlock"
            | "DeletePublicAccessBlock"
            | "GetBucketPolicyStatus"
            | "GetObjectLockConfiguration"
            | "PutObjectLockConfiguration"
            | "DeleteObjects"
            | "ListMultipartUploads"
            | "CreateSession" => {
                let bucket = input.get("Bucket").and_then(|v| v.as_str())?;
                Some(format!("arn:aws:s3:::{bucket}"))
            }
            _ => {
                let bucket = input.get("Bucket").and_then(|v| v.as_str())?;
                let key = input.get("Key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    Some(format!("arn:aws:s3:::{bucket}"))
                } else {
                    Some(format!("arn:aws:s3:::{bucket}/{key}"))
                }
            }
        }
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        use awsim_core::Snapshottable;
        use state::S3RegionSnapshot;

        if let Ok(()) = self.store.restore_from_bytes(data) {
            self.rebind_bodies();
            return Ok(());
        }

        let legacy: S3StateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let region_snap = S3RegionSnapshot {
            account_id: "000000000000".to_string(),
            region: "global".to_string(),
            buckets: legacy.buckets,
        };
        let (acct, region, state) = S3State::from_snapshot(region_snap);
        self.store.clear();
        self.store.set(&acct, &region, state);
        self.rebind_bodies();
        Ok(())
    }
}

impl S3Service {
    fn rebind_bodies(&self) {
        use awsim_core::Body;

        let Some(bs) = &self.body_store else {
            return;
        };
        for (_, state) in self.store.iter_all() {
            state.set_body_store(Arc::clone(bs));
            for mut bucket_entry in state.buckets.iter_mut() {
                let bucket_name = bucket_entry.key().clone();
                for mut obj_entry in bucket_entry.value_mut().objects.iter_mut() {
                    let key = obj_entry.key().clone();
                    for version in obj_entry.value_mut().versions.iter_mut() {
                        if version.is_delete_marker {
                            continue;
                        }
                        let blob_key = operations::object::versioned_blob_key(
                            &key,
                            version.version_id.as_deref(),
                        );
                        match bs.blob_path("objects", &bucket_name, &blob_key) {
                            Ok(path) if path.exists() => version.body = Body::OnDisk(path),
                            // Fall back to the legacy un-versioned blob path
                            // so snapshots written before per-version blobs
                            // are still readable on restore.
                            Ok(_) => match bs.blob_path("objects", &bucket_name, &key) {
                                Ok(legacy) if legacy.exists() => {
                                    version.body = Body::OnDisk(legacy);
                                }
                                _ => {}
                            },
                            Err(e) => tracing::warn!(
                                bucket = %bucket_name,
                                key = %key,
                                version = ?version.version_id,
                                error = %e,
                                "resolve object path on restore"
                            ),
                        }
                    }
                }
            }
        }
    }
}
