//! AWS X-Ray emulator. Accepts trace segments via PutTraceSegments, persists
//! them in memory, and serves the listing/aggregation operations the SDK and
//! the X-Ray daemon hit when populating the console.

mod operations;
pub mod state;

pub use state::XrayState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct XrayService {
    store: AccountRegionStore<XrayState>,
}

impl XrayService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<XrayState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<XrayState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for XrayService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for XrayService {
    fn service_name(&self) -> &str {
        "xray"
    }

    fn signing_name(&self) -> &str {
        "xray"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/TraceSegments",
                operation: "PutTraceSegments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/Traces",
                operation: "BatchGetTraces",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/TraceSummaries",
                operation: "GetTraceSummaries",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/ServiceGraph",
                operation: "GetServiceGraph",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/GetSamplingRules",
                operation: "GetSamplingRules",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/CreateSamplingRule",
                operation: "CreateSamplingRule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/DeleteSamplingRule",
                operation: "DeleteSamplingRule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/SamplingTargets",
                operation: "GetSamplingTargets",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/CreateGroup",
                operation: "CreateGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/DeleteGroup",
                operation: "DeleteGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/Groups",
                operation: "GetGroups",
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
        debug!(operation, "X-Ray request");
        let state = self.get_state(ctx);
        match operation {
            "PutTraceSegments" => operations::put_trace_segments(&state, &input, ctx),
            "BatchGetTraces" => operations::batch_get_traces(&state, &input, ctx),
            "GetTraceSummaries" => operations::get_trace_summaries(&state, &input, ctx),
            "GetServiceGraph" => operations::get_service_graph(&state, &input, ctx),
            "GetSamplingRules" => operations::get_sampling_rules(&state, &input, ctx),
            "CreateSamplingRule" => operations::create_sampling_rule(&state, &input, ctx),
            "DeleteSamplingRule" => operations::delete_sampling_rule(&state, &input, ctx),
            "GetSamplingTargets" => operations::get_sampling_targets(&state, &input, ctx),
            "CreateGroup" => operations::create_group(&state, &input, ctx),
            "DeleteGroup" => operations::delete_group(&state, &input, ctx),
            "GetGroups" => operations::get_groups(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::XrayStateSnapshot {
            traces: vec![],
            sampling_rules: Default::default(),
            groups: Default::default(),
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.traces.extend(s.traces);
            all.sampling_rules.extend(s.sampling_rules);
            all.groups.extend(s.groups);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::XrayStateSnapshot =
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
        RequestContext::new("xray", "us-east-1")
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
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    fn segment_doc(trace_id: &str, name: &str, start: f64, end: f64, fault: bool) -> String {
        serde_json::json!({
            "trace_id": trace_id,
            "id": "1234567890123456",
            "name": name,
            "start_time": start,
            "end_time": end,
            "fault": fault,
        })
        .to_string()
    }

    #[test]
    fn put_then_summarize_and_graph() {
        let svc = XrayService::new();
        let ctx = ctx();

        let put = block_on(svc.handle(
            "PutTraceSegments",
            json!({
                "TraceSegmentDocuments": [
                    segment_doc("1-65f5a8a0-1234567890abcdef12345678", "checkout-svc", 100.0, 100.5, false),
                    segment_doc("1-65f5a8a0-1234567890abcdef12345678", "payment-svc", 100.1, 100.4, true),
                ]
            }),
            &ctx,
        ))
        .unwrap();
        assert!(
            put["UnprocessedTraceSegments"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let summaries = block_on(svc.handle(
            "GetTraceSummaries",
            json!({ "StartTime": 0, "EndTime": 9_999_999_999.0 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(summaries["TraceSummaries"].as_array().unwrap().len(), 1);
        let s = &summaries["TraceSummaries"][0];
        assert_eq!(s["HasFault"], true);
        assert!(s["Duration"].as_f64().unwrap() >= 0.4);

        let graph = block_on(svc.handle("GetServiceGraph", json!({}), &ctx)).unwrap();
        let svcs = graph["Services"].as_array().unwrap();
        assert_eq!(svcs.len(), 2);

        let traces = block_on(svc.handle(
            "BatchGetTraces",
            json!({ "TraceIds": ["1-65f5a8a0-1234567890abcdef12345678"] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(traces["Traces"].as_array().unwrap().len(), 1);
        assert_eq!(traces["Traces"][0]["Segments"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unprocessed_segments_returned_for_invalid_input() {
        let svc = XrayService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "PutTraceSegments",
            json!({ "TraceSegmentDocuments": ["{not json", json!({"id": "x"}).to_string()] }),
            &ctx,
        ))
        .unwrap();
        let unp = r["UnprocessedTraceSegments"].as_array().unwrap();
        assert_eq!(unp.len(), 2);
    }
}
