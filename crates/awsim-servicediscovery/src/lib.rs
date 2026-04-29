//! AWS Cloud Map (Service Discovery) emulator. Stores namespaces, services,
//! and instances; every async operation collapses to `SUCCESS` immediately so
//! callers don't have to poll.

mod operations;
pub mod state;

pub use state::ServiceDiscoveryState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct ServiceDiscoveryService {
    store: AccountRegionStore<ServiceDiscoveryState>,
}

impl ServiceDiscoveryService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<ServiceDiscoveryState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<ServiceDiscoveryState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for ServiceDiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for ServiceDiscoveryService {
    fn service_name(&self) -> &str {
        "servicediscovery"
    }

    fn signing_name(&self) -> &str {
        "servicediscovery"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "ServiceDiscovery request");
        let state = self.get_state(ctx);
        match operation {
            "CreateHttpNamespace" => operations::create_http_namespace(&state, &input, ctx),
            "CreatePrivateDnsNamespace" => {
                operations::create_private_dns_namespace(&state, &input, ctx)
            }
            "CreatePublicDnsNamespace" => {
                operations::create_public_dns_namespace(&state, &input, ctx)
            }
            "DeleteNamespace" => operations::delete_namespace(&state, &input, ctx),
            "GetNamespace" => operations::get_namespace(&state, &input, ctx),
            "ListNamespaces" => operations::list_namespaces(&state, &input, ctx),
            "CreateService" => operations::create_service(&state, &input, ctx),
            "DeleteService" => operations::delete_service(&state, &input, ctx),
            "GetService" => operations::get_service(&state, &input, ctx),
            "ListServices" => operations::list_services(&state, &input, ctx),
            "RegisterInstance" => operations::register_instance(&state, &input, ctx),
            "DeregisterInstance" => operations::deregister_instance(&state, &input, ctx),
            "GetInstance" => operations::get_instance(&state, &input, ctx),
            "ListInstances" => operations::list_instances(&state, &input, ctx),
            "DiscoverInstances" => operations::discover_instances(&state, &input, ctx),
            "GetOperation" => operations::get_operation(&state, &input, ctx),
            "ListOperations" => operations::list_operations(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::ServiceDiscoverySnapshot {
            namespaces: vec![],
            services: vec![],
            instances: vec![],
            operations: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.namespaces.extend(s.namespaces);
            all.services.extend(s.services);
            all.instances.extend(s.instances);
            all.operations.extend(s.operations);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::ServiceDiscoverySnapshot =
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
        RequestContext::new("servicediscovery", "us-east-1")
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
    fn ns_service_instance_lifecycle() {
        let svc = ServiceDiscoveryService::new();
        let ctx = ctx();

        // Create namespace
        let n_op = block_on(svc.handle("CreateHttpNamespace", json!({ "Name": "internal" }), &ctx))
            .unwrap();
        assert!(n_op["OperationId"].as_str().is_some());

        let nss = block_on(svc.handle("ListNamespaces", json!({}), &ctx)).unwrap();
        let ns_id = nss["Namespaces"][0]["Id"].as_str().unwrap().to_string();

        // Create service
        let s = block_on(svc.handle(
            "CreateService",
            json!({ "Name": "checkout", "NamespaceId": ns_id }),
            &ctx,
        ))
        .unwrap();
        let svc_id = s["Service"]["Id"].as_str().unwrap().to_string();

        // Register instance
        block_on(svc.handle(
            "RegisterInstance",
            json!({
                "ServiceId": svc_id,
                "InstanceId": "task-1",
                "Attributes": { "AWS_INSTANCE_IPV4": "10.0.0.5", "AWS_INSTANCE_PORT": "8080" }
            }),
            &ctx,
        ))
        .unwrap();

        // Discover by names
        let discovered = block_on(svc.handle(
            "DiscoverInstances",
            json!({ "NamespaceName": "internal", "ServiceName": "checkout" }),
            &ctx,
        ))
        .unwrap();
        let insts = discovered["Instances"].as_array().unwrap();
        assert_eq!(insts.len(), 1);
        assert_eq!(insts[0]["Attributes"]["AWS_INSTANCE_IPV4"], "10.0.0.5");
        assert_eq!(insts[0]["HealthStatus"], "HEALTHY");

        // Service.instance_count was bumped
        let described = block_on(svc.handle("GetService", json!({ "Id": svc_id }), &ctx)).unwrap();
        assert_eq!(described["Service"]["InstanceCount"], 1);
    }

    #[test]
    fn delete_blocks_when_resources_remain() {
        let svc = ServiceDiscoveryService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateHttpNamespace", json!({ "Name": "ns" }), &ctx)).unwrap();
        let ns_id = block_on(svc.handle("ListNamespaces", json!({}), &ctx)).unwrap()["Namespaces"]
            [0]["Id"]
            .as_str()
            .unwrap()
            .to_string();
        block_on(svc.handle(
            "CreateService",
            json!({ "Name": "s", "NamespaceId": ns_id }),
            &ctx,
        ))
        .unwrap();
        let err =
            block_on(svc.handle("DeleteNamespace", json!({ "Id": ns_id }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ResourceInUse");
    }
}
