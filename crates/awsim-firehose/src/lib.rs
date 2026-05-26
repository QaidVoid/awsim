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
}
