mod operations;
mod state;
mod util;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::{Bucket, S3State, S3StateSnapshot};

/// The AWSim S3 service handler.
pub struct S3Service {
    store: AccountRegionStore<S3State>,
}

impl S3Service {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<S3State> {
        // S3 state is global per account — region is not used for state namespacing.
        self.store.get(&ctx.account_id, "global")
    }
}

impl Default for S3Service {
    fn default() -> Self {
        Self::new()
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
            // GET /{Bucket}?list-type=2
            RouteDefinition {
                method: "GET",
                path_pattern: "/{Bucket}",
                operation: "ListObjectsV2",
                required_query_param: Some("list-type"),
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
            "PutBucketVersioning" => operations::config::put_bucket_versioning(&state, &input),
            "GetBucketVersioning" => operations::config::get_bucket_versioning(&state, &input),
            "PutBucketPolicy" => operations::config::put_bucket_policy(&state, &input),
            "GetBucketPolicy" => operations::config::get_bucket_policy(&state, &input),
            "DeleteBucketPolicy" => operations::config::delete_bucket_policy(&state, &input),
            "PutBucketCors" => operations::config::put_bucket_cors(&state, &input),
            "GetBucketCors" => operations::config::get_bucket_cors(&state, &input),
            "DeleteBucketCors" => operations::config::delete_bucket_cors(&state, &input),

            // Object operations
            "PutObject" | "CopyObject" => operations::object::put_object(&state, &input, ctx),
            "GetObject" => operations::object::get_object(&state, &input, ctx),
            "HeadObject" => operations::object::head_object(&state, &input),
            "DeleteObject" => operations::object::delete_object(&state, &input),

            // Listing / batch
            "ListObjectsV2" => operations::list::list_objects_v2(&state, &input),
            "DeleteObjects" => operations::list::delete_objects(&state, &input),

            // Multipart
            "CreateMultipartUpload" => {
                operations::multipart::create_multipart_upload(&state, &input)
            }
            "UploadPart" => operations::multipart::upload_part(&state, &input),
            "CompleteMultipartUpload" => {
                operations::multipart::complete_multipart_upload(&state, &input)
            }
            "AbortMultipartUpload" => {
                operations::multipart::abort_multipart_upload(&state, &input)
            }
            "ListMultipartUploads" => {
                operations::multipart::list_multipart_uploads(&state, &input)
            }
            "ListParts" => operations::multipart::list_parts(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        use state::{BucketSnapshot, S3ObjectMetadata};

        let buckets: Vec<BucketSnapshot> = self
            .store
            .iter_all()
            .into_iter()
            .flat_map(|(_, state)| {
                state
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
                            // Persist object metadata only — no data bytes
                            objects: b
                                .objects
                                .iter()
                                .map(|oe| S3ObjectMetadata::from(oe.value()))
                                .collect(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        serde_json::to_vec(&S3StateSnapshot { buckets }).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        use dashmap::DashMap;
        use state::S3Object;

        let snapshot: S3StateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        // S3 state is global per account — always use "global" region.
        let state = self.store.get("000000000000", "global");

        for bs in snapshot.buckets {
            let bucket = Bucket {
                name: bs.name.clone(),
                region: bs.region.clone(),
                created_at: bs.created_at.clone(),
                versioning: bs.versioning,
                tags: bs.tags,
                policy: bs.policy,
                cors: bs.cors,
                objects: {
                    let dm = DashMap::new();
                    for meta in bs.objects {
                        // Restore metadata; data is empty — object data is not persisted
                        dm.insert(
                            meta.key.clone(),
                            S3Object {
                                key: meta.key,
                                data: Vec::new(), // not persisted
                                content_type: meta.content_type,
                                content_length: meta.content_length,
                                etag: meta.etag,
                                last_modified: meta.last_modified,
                                metadata: meta.metadata,
                                version_id: meta.version_id,
                            },
                        );
                    }
                    dm
                },
                multipart_uploads: DashMap::new(), // not persisted
            };
            state.buckets.insert(bs.name, bucket);
        }

        Ok(())
    }
}
