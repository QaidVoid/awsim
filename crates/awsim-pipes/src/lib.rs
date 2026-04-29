//! EventBridge Pipes emulator.
//!
//! Implements the data-plane state for Pipes (Create/Describe/List/Update/
//! Delete/Start/Stop) with RestJson1 routes. Actual source→target dispatch
//! is driven by a separate background runner spawned in the awsim binary.

mod operations;
pub mod state;

pub use state::{Pipe, PipesState};

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct PipesService {
    store: AccountRegionStore<PipesState>,
}

impl PipesService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<PipesState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<PipesState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for PipesService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for PipesService {
    fn service_name(&self) -> &str {
        "pipes"
    }

    fn signing_name(&self) -> &str {
        "pipes"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/pipes/{Name}",
                operation: "CreatePipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/pipes/{Name}",
                operation: "DescribePipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/pipes",
                operation: "ListPipes",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/pipes/{Name}",
                operation: "DeletePipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/pipes/{Name}",
                operation: "UpdatePipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/pipes/{Name}/start",
                operation: "StartPipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/pipes/{Name}/stop",
                operation: "StopPipe",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{ResourceArn}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{ResourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{ResourceArn}",
                operation: "UntagResource",
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
        debug!(operation, "Pipes request");
        let state = self.get_state(ctx);
        match operation {
            "CreatePipe" => operations::pipes::create_pipe(&state, &input, ctx),
            "DescribePipe" => operations::pipes::describe_pipe(&state, &input, ctx),
            "ListPipes" => operations::pipes::list_pipes(&state, &input, ctx),
            "DeletePipe" => operations::pipes::delete_pipe(&state, &input, ctx),
            "UpdatePipe" => operations::pipes::update_pipe(&state, &input, ctx),
            "StartPipe" => operations::pipes::start_pipe(&state, &input, ctx),
            "StopPipe" => operations::pipes::stop_pipe(&state, &input, ctx),
            "ListTagsForResource" => operations::pipes::list_tags_for_resource(&state, &input, ctx),
            "TagResource" => operations::tags::tag_resource(&state, &input, ctx),
            "UntagResource" => operations::tags::untag_resource(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::PipesStateSnapshot { pipes: vec![] };
        for (_, st) in self.store.iter_all() {
            all.pipes.extend(st.to_snapshot().pipes);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::PipesStateSnapshot =
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
        RequestContext::new("pipes", "us-east-1")
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

    #[test]
    fn create_describe_list_delete_lifecycle() {
        let svc = PipesService::new();
        let ctx = ctx();

        let created = block_on(svc.handle(
            "CreatePipe",
            json!({
                "Name": "orders-pipe",
                "Source": "arn:aws:sqs:us-east-1:000000000000:orders",
                "Target": "arn:aws:lambda:us-east-1:000000000000:function:processor",
                "RoleArn": "arn:aws:iam::000000000000:role/PipesRole",
                "DesiredState": "RUNNING",
                "Description": "test pipe",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(created["CurrentState"], "RUNNING");
        assert!(
            created["Arn"]
                .as_str()
                .unwrap()
                .ends_with(":pipe/orders-pipe")
        );

        let list = block_on(svc.handle("ListPipes", json!({}), &ctx)).unwrap();
        assert_eq!(list["Pipes"].as_array().unwrap().len(), 1);

        let stopped =
            block_on(svc.handle("StopPipe", json!({ "Name": "orders-pipe" }), &ctx)).unwrap();
        assert_eq!(stopped["CurrentState"], "STOPPED");

        let described =
            block_on(svc.handle("DescribePipe", json!({ "Name": "orders-pipe" }), &ctx)).unwrap();
        assert_eq!(described["Description"], "test pipe");

        let deleted =
            block_on(svc.handle("DeletePipe", json!({ "Name": "orders-pipe" }), &ctx)).unwrap();
        assert_eq!(deleted["CurrentState"], "DELETING");

        let after = block_on(svc.handle("ListPipes", json!({}), &ctx)).unwrap();
        assert!(after["Pipes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn create_duplicate_pipe_conflicts() {
        let svc = PipesService::new();
        let ctx = ctx();
        let body = json!({
            "Name": "p",
            "Source": "arn:aws:sqs:us-east-1:000000000000:q",
            "Target": "arn:aws:lambda:us-east-1:000000000000:function:f",
            "RoleArn": "arn:aws:iam::000000000000:role/r",
        });
        block_on(svc.handle("CreatePipe", body.clone(), &ctx)).unwrap();
        let err = block_on(svc.handle("CreatePipe", body, &ctx)).unwrap_err();
        assert_eq!(err.code, "ConflictException");
    }
}
