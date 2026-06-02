mod delivery;
pub mod error;
mod operations;
mod processors;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, LambdaInvoker, Protocol, RequestContext, S3ObjectWriter,
    ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::FirehoseState;

pub struct FirehoseService {
    store: AccountRegionStore<FirehoseState>,
    /// In-process S3 writer used to deliver buffered records; `None`
    /// disables delivery (e.g. in unit tests).
    s3_writer: Option<Arc<dyn S3ObjectWriter>>,
    /// Lambda invoker used to run data-transformation processors.
    lambda_invoker: Option<Arc<dyn LambdaInvoker>>,
}

impl FirehoseService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            s3_writer: None,
            lambda_invoker: None,
        }
    }

    /// Wire the in-process S3 writer so delivery to (Extended)S3
    /// destinations actually lands objects in the embedded S3.
    pub fn with_s3_writer(mut self, writer: Arc<dyn S3ObjectWriter>) -> Self {
        self.s3_writer = Some(writer);
        self
    }

    /// Wire the Lambda invoker so configured data-transformation
    /// processors run before delivery.
    pub fn with_lambda_invoker(mut self, invoker: Arc<dyn LambdaInvoker>) -> Self {
        self.lambda_invoker = Some(invoker);
        self
    }

    /// Extract the base64 `Data` payloads from a PutRecord / PutRecordBatch
    /// input and deliver them to the stream's S3 destination.
    fn deliver_put(&self, state: &FirehoseState, input: &Value, ctx: &RequestContext) {
        let Some(name) = input["DeliveryStreamName"].as_str() else {
            return;
        };
        let records: Vec<String> = if let Some(rec) = input.get("Record") {
            rec.get("Data")
                .and_then(Value::as_str)
                .map(|d| vec![d.to_string()])
                .unwrap_or_default()
        } else {
            input
                .get("Records")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| r.get("Data").and_then(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        delivery::deliver_records(
            state,
            self.s3_writer.as_ref(),
            self.lambda_invoker.as_ref(),
            name,
            &records,
            &ctx.account_id,
            &ctx.region,
        );
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
            "PutRecord" => {
                let resp = operations::records::put_record(&state, &input, ctx)?;
                self.deliver_put(&state, &input, ctx);
                Ok(resp)
            }
            "PutRecordBatch" => {
                let resp = operations::records::put_record_batch(&state, &input, ctx)?;
                self.deliver_put(&state, &input, ctx);
                Ok(resp)
            }
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

    /// Advance each stream's encryption state machine (ENABLING ->
    /// ENABLED, DISABLING -> DISABLED) across all tenants.
    async fn tick(&self) {
        for (_, state) in self.store.iter_all() {
            for mut entry in state.streams.iter_mut() {
                entry.value_mut().advance_encryption();
            }
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
    use std::sync::Mutex;

    fn ctx() -> RequestContext {
        RequestContext::new("firehose", "us-east-1")
    }

    /// Records every put_object call so delivery routing can be asserted
    /// without a real S3.
    struct RecordingWriter {
        calls: Mutex<Vec<(String, String)>>, // (bucket, key)
    }
    impl S3ObjectWriter for RecordingWriter {
        fn put_object(
            &self,
            bucket: &str,
            key: &str,
            _body_b64: &str,
            _account: &str,
            _region: &str,
        ) -> Result<(), AwsError> {
            self.calls
                .lock()
                .unwrap()
                .push((bucket.to_string(), key.to_string()));
            Ok(())
        }
    }

    /// Returns a fixed transform response: first record Ok, second
    /// ProcessingFailed.
    struct SplittingInvoker;
    impl LambdaInvoker for SplittingInvoker {
        fn invoke(
            &self,
            _function_name: &str,
            _payload: &Value,
            _account: &str,
            _region: &str,
        ) -> Result<Value, AwsError> {
            Ok(json!({ "records": [
                { "recordId": "r0", "result": "Ok", "data": "dHJhbnNmb3JtZWQ=" },
                { "recordId": "r1", "result": "ProcessingFailed" },
            ]}))
        }
    }

    #[test]
    fn put_record_batch_delivers_and_routes_failures_to_error_prefix() {
        let writer = Arc::new(RecordingWriter {
            calls: Mutex::new(Vec::new()),
        });
        let svc = FirehoseService::new()
            .with_s3_writer(writer.clone())
            .with_lambda_invoker(Arc::new(SplittingInvoker));
        let ctx = ctx();
        block_on(svc.handle(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": "deliver-me",
                "ExtendedS3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::logs-bucket",
                    "Prefix": "raw/",
                    "ErrorOutputPrefix": "err/",
                    "FileExtension": ".json",
                    "ProcessingConfiguration": {
                        "Enabled": true,
                        "Processors": [{
                            "Type": "Lambda",
                            "Parameters": [{
                                "ParameterName": "LambdaArn",
                                "ParameterValue": "arn:aws:lambda:us-east-1:000000000000:function:t",
                            }],
                        }],
                    },
                },
            }),
            &ctx,
        ))
        .unwrap();

        block_on(svc.handle(
            "PutRecordBatch",
            json!({
                "DeliveryStreamName": "deliver-me",
                "Records": [{ "Data": "aGVsbG8=" }, { "Data": "d29ybGQ=" }],
            }),
            &ctx,
        ))
        .unwrap();

        let calls = writer.calls.lock().unwrap().clone();
        assert_eq!(
            calls.len(),
            2,
            "expected a main + an error object, got {calls:?}"
        );
        assert!(calls.iter().all(|(b, _)| b == "logs-bucket"));
        let main_key = calls
            .iter()
            .find(|(_, k)| k.starts_with("raw/"))
            .expect("main object");
        let err_key = calls
            .iter()
            .find(|(_, k)| k.starts_with("err/"))
            .expect("error object");
        assert!(main_key.1.contains("/deliver-me-"));
        assert!(main_key.1.ends_with(".json"));
        assert!(err_key.1.contains("/deliver-me-"));

        let stats = svc
            .store
            .get(&ctx.account_id, &ctx.region)
            .delivery_stats
            .get("deliver-me")
            .map(|s| s.clone())
            .unwrap();
        assert_eq!(stats.succeeded_records, 1);
        assert_eq!(stats.processing_failed, 1);
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
        // Encryption is async (ENABLING -> ENABLED); advance the tick once.
        block_on(svc.tick());
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
