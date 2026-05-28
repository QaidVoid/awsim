pub mod error;
mod operations;
mod state;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::FirehoseState;

pub struct FirehoseService {
    store: AccountRegionStore<FirehoseState>,
}

impl FirehoseService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for FirehoseService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for FirehoseService {
    fn service_name(&self) -> &str {
        "firehose"
    }

    fn signing_name(&self) -> &str {
        "firehose"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "Firehose operation");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateDeliveryStream" => {
                operations::streams::create_delivery_stream(&state, &input, ctx)
            }
            "DeleteDeliveryStream" => {
                operations::streams::delete_delivery_stream(&state, &input, ctx)
            }
            "DescribeDeliveryStream" => {
                operations::streams::describe_delivery_stream(&state, &input, ctx)
            }
            "ListDeliveryStreams" => {
                operations::streams::list_delivery_streams(&state, &input, ctx)
            }
            "UpdateDestination" => operations::streams::update_destination(&state, &input, ctx),
            "PutRecord" => operations::records::put_record(&state, &input, ctx),
            "PutRecordBatch" => operations::records::put_record_batch(&state, &input, ctx),
            "TagDeliveryStream" => operations::tags::tag_delivery_stream(&state, &input, ctx),
            "UntagDeliveryStream" => operations::tags::untag_delivery_stream(&state, &input, ctx),
            "ListTagsForDeliveryStream" => {
                operations::tags::list_tags_for_delivery_stream(&state, &input, ctx)
            }
            "StartDeliveryStreamEncryption" => {
                operations::encryption::start_encryption(&state, &input, ctx)
            }
            "StopDeliveryStreamEncryption" => {
                operations::encryption::stop_encryption(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::FirehoseSnapshot { streams: vec![] };
        for (_, st) in self.store.iter_all() {
            all.streams.extend(st.to_snapshot().streams);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::FirehoseSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("firehose", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn create_delivery_stream_rejects_unknown_type() {
        let svc = FirehoseService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateDeliveryStream",
            json!({ "DeliveryStreamName": "bad", "DeliveryStreamType": "MAGIC" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_delivery_stream_rejects_invalid_compression() {
        let svc = FirehoseService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": "bad-compression",
                "ExtendedS3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::b",
                    "CompressionFormat": "BROTLI"
                }
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn create_delivery_stream_rejects_buffering_size_out_of_range() {
        let svc = FirehoseService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": "bad-buf",
                "ExtendedS3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::b",
                    "BufferingHints": { "SizeInMBs": 1024, "IntervalInSeconds": 300 }
                }
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidArgumentException");
    }

    #[test]
    fn snapshot_round_trips_streams_with_encryption_and_version() {
        let svc = FirehoseService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": "snap-roundtrip",
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::b" },
            }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "StartDeliveryStreamEncryption",
            json!({
                "DeliveryStreamName": "snap-roundtrip",
                "DeliveryStreamEncryptionConfigurationInput": { "KeyType": "AWS_OWNED_CMK" },
            }),
            &ctx,
        ))
        .unwrap();
        let described = block_on(svc.handle(
            "DescribeDeliveryStream",
            json!({ "DeliveryStreamName": "snap-roundtrip" }),
            &ctx,
        ))
        .unwrap();
        let destination_id =
            described["DeliveryStreamDescription"]["Destinations"][0]["DestinationId"]
                .as_str()
                .unwrap()
                .to_string();
        block_on(svc.handle(
            "UpdateDestination",
            json!({
                "DeliveryStreamName": "snap-roundtrip",
                "CurrentDeliveryStreamVersionId": "1",
                "DestinationId": destination_id,
                "ExtendedS3DestinationConfiguration": { "BucketARN": "arn:aws:s3:::c" },
            }),
            &ctx,
        ))
        .unwrap();

        let bytes = svc.snapshot().expect("snapshot encodes");
        let restored = FirehoseService::new();
        restored.restore(&bytes).expect("restore succeeds");
        let desc = block_on(restored.handle(
            "DescribeDeliveryStream",
            json!({ "DeliveryStreamName": "snap-roundtrip" }),
            &ctx,
        ))
        .unwrap();
        let info = &desc["DeliveryStreamDescription"];
        assert_eq!(info["VersionId"], "2");
        assert_eq!(
            info["DeliveryStreamEncryptionConfiguration"]["Status"],
            "ENABLED"
        );
        assert_eq!(
            info["DeliveryStreamEncryptionConfiguration"]["KeyType"],
            "AWS_OWNED_CMK"
        );
    }
}
