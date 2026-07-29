//! Amazon MQ emulator. Brokers, broker users, and configuration metadata.
//! The emulator never spins up real ActiveMQ/RabbitMQ. A broker comes up
//! `CREATION_IN_PROGRESS` and settles to `RUNNING` once its transition
//! deadline elapses (driven by `tick` or by polling `DescribeBroker`); the
//! default create delay is `0`, so it promotes on the first describe.

mod operations;
pub mod state;

pub use state::MqState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct MqService {
    store: AccountRegionStore<MqState>,
}

impl MqService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<MqState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<MqState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// Count active brokers for a given account+region. Used by the
    /// billing meter to charge broker-hours. AWS bills any broker
    /// that's running or in a transitional state that still costs
    /// money (`CREATION_IN_PROGRESS`, `RUNNING`, `REBOOT_IN_PROGRESS`).
    pub fn running_broker_count(&self, account_id: &str, region: &str) -> u64 {
        let state = self.store.get(account_id, region);
        state
            .brokers
            .iter()
            .filter(|b| {
                matches!(
                    b.value().broker_state.as_str(),
                    "RUNNING" | "CREATION_IN_PROGRESS" | "REBOOT_IN_PROGRESS"
                )
            })
            .count() as u64
    }
}

impl Default for MqService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for MqService {
    fn service_name(&self) -> &str {
        "mq"
    }

    fn signing_name(&self) -> &str {
        "mq"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/brokers",
                operation: "CreateBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers",
                operation: "ListBrokers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers/{brokerId}",
                operation: "DescribeBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/brokers/{brokerId}",
                operation: "DeleteBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/brokers/{brokerId}",
                operation: "UpdateBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/brokers/{brokerId}/reboot",
                operation: "RebootBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/brokers/{brokerId}/users/{username}",
                operation: "CreateUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers/{brokerId}/users/{username}",
                operation: "DescribeUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers/{brokerId}/users",
                operation: "ListUsers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/brokers/{brokerId}/users/{username}",
                operation: "DeleteUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/brokers/{brokerId}/users/{username}",
                operation: "UpdateUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/configurations",
                operation: "CreateConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/configurations",
                operation: "ListConfigurations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/configurations/{configurationId}",
                operation: "DescribeConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/configurations/{configurationId}",
                operation: "UpdateConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/configurations/{configurationId}/revisions/{ConfigurationRevision}",
                operation: "DescribeConfigurationRevision",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "CreateTags",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "DeleteTags",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/tags/{resourceArn}",
                operation: "ListTags",
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
        debug!(operation, "MQ request");
        let state = self.get_state(ctx);
        match operation {
            "CreateBroker" => operations::create_broker(&state, &input, ctx),
            "DescribeBroker" => operations::describe_broker(&state, &input, ctx),
            "ListBrokers" => operations::list_brokers(&state, &input, ctx),
            "DeleteBroker" => operations::delete_broker(&state, &input, ctx),
            "UpdateBroker" => operations::update_broker(&state, &input, ctx),
            "RebootBroker" => operations::reboot_broker(&state, &input, ctx),
            "CreateUser" => operations::create_user(&state, &input, ctx),
            "DescribeUser" => operations::describe_user(&state, &input, ctx),
            "ListUsers" => operations::list_users(&state, &input, ctx),
            "DeleteUser" => operations::delete_user(&state, &input, ctx),
            "UpdateUser" => operations::update_user(&state, &input, ctx),
            "CreateConfiguration" => operations::create_configuration(&state, &input, ctx),
            "UpdateConfiguration" => operations::update_configuration(&state, &input, ctx),
            "DescribeConfiguration" => operations::describe_configuration(&state, &input, ctx),
            "DescribeConfigurationRevision" => {
                operations::describe_configuration_revision(&state, &input, ctx)
            }
            "ListConfigurations" => operations::list_configurations(&state, &input, ctx),
            "CreateTags" => operations::create_tags(&state, &input, ctx),
            "DeleteTags" => operations::delete_tags(&state, &input, ctx),
            "ListTags" => operations::list_tags(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    /// Drive the broker state machine. Any broker whose transition
    /// deadline (`state_at`) has elapsed flips to `RUNNING`. Absolute
    /// wall-clock gated and idempotent, so a missed or repeated tick
    /// never loses or double-applies state. `DescribeBroker` runs the
    /// same promotion on poll, so callers that don't run a tick loop
    /// still observe the broker settle.
    async fn tick(&self) {
        let now = operations::now();
        for (_, state) in self.store.iter_all() {
            for mut entry in state.brokers.iter_mut() {
                operations::promote_if_due(entry.value_mut(), now);
            }
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = state::MqSnapshot {
            brokers: vec![],
            users: vec![],
            configurations: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.brokers.extend(s.brokers);
            all.users.extend(s.users);
            all.configurations.extend(s.configurations);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::MqSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
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
        RequestContext::new("mq", "us-east-1")
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
    fn broker_with_initial_user_lifecycle() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "primary",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "deploymentMode": "SINGLE_INSTANCE",
                "users": [{ "username": "admin", "consoleAccess": true }]
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();

        let described =
            block_on(svc.handle("DescribeBroker", json!({ "brokerId": id }), &ctx)).unwrap();
        assert_eq!(described["brokerState"], "RUNNING");
        assert_eq!(described["users"].as_array().unwrap().len(), 1);
        assert_eq!(described["engineType"], "RABBITMQ");

        block_on(svc.handle(
            "CreateUser",
            json!({ "brokerId": id, "username": "app", "password": "x" }),
            &ctx,
        ))
        .unwrap();
        let users = block_on(svc.handle("ListUsers", json!({ "brokerId": id }), &ctx)).unwrap();
        assert_eq!(users["users"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn create_broker_rejects_invalid_name() {
        let svc = MqService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "bad name!",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.t3.micro"
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn create_broker_rejects_rabbitmq_with_efs_storage() {
        let svc = MqService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "rmq",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "storageType": "EFS"
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn create_broker_requires_ldap_metadata_when_strategy_is_ldap() {
        let svc = MqService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "ldap-broker",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.t3.micro",
                "authenticationStrategy": "LDAP"
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
        assert!(err.message.contains("LdapServerMetadata"));
    }

    #[test]
    fn duplicate_broker_name_rejected() {
        let svc = MqService::new();
        let ctx = ctx();
        let body = json!({
            "brokerName": "dup",
            "engineType": "ACTIVEMQ",
            "engineVersion": "5.18",
            "hostInstanceType": "mq.t3.micro"
        });
        block_on(svc.handle("CreateBroker", body.clone(), &ctx)).unwrap();
        let err = block_on(svc.handle("CreateBroker", body, &ctx)).unwrap_err();
        assert_eq!(err.code, "ConflictException");
    }

    fn make_broker_with_user(svc: &MqService, ctx: &RequestContext) -> String {
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "secrets",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.t3.micro",
                "users": [{
                    "username": "alice",
                    "consoleAccess": false,
                    "groups": ["g1"],
                    "password": "hunter2"
                }]
            }),
            ctx,
        ))
        .unwrap();
        r["brokerId"].as_str().unwrap().to_string()
    }

    #[test]
    fn describe_user_does_not_surface_password() {
        let svc = MqService::new();
        let ctx = ctx();
        let id = make_broker_with_user(&svc, &ctx);
        let desc = block_on(svc.handle(
            "DescribeUser",
            json!({ "brokerId": id, "username": "alice" }),
            &ctx,
        ))
        .unwrap();
        let serialized = desc.to_string();
        assert!(
            !serialized.contains("hunter2"),
            "plaintext leaked: {serialized}"
        );
        assert!(
            !serialized.contains("Password"),
            "Password field must be absent: {serialized}"
        );
    }

    #[test]
    fn update_user_writes_to_pending_mirror() {
        let svc = MqService::new();
        let ctx = ctx();
        let id = make_broker_with_user(&svc, &ctx);
        block_on(svc.handle(
            "UpdateUser",
            json!({
                "brokerId": id,
                "username": "alice",
                "consoleAccess": true,
                "groups": ["admins"],
            }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle(
            "DescribeUser",
            json!({ "brokerId": id, "username": "alice" }),
            &ctx,
        ))
        .unwrap();
        // Live values unchanged.
        assert_eq!(desc["consoleAccess"], false);
        assert_eq!(desc["groups"][0], "g1");
        // Pending mirror reflects the requested update.
        let pending = desc["pending"].as_object().expect("Pending populated");
        assert_eq!(pending["consoleAccess"], true);
        assert_eq!(pending["groups"][0], "admins");
    }

    #[test]
    fn creator_request_id_replays_cached_response() {
        let svc = MqService::new();
        let ctx = ctx();
        let first = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "idemp-broker",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "creatorRequestId": "token-abc",
            }),
            &ctx,
        ))
        .unwrap();
        let id_first = first["brokerId"].as_str().unwrap().to_string();

        // Same token + same body must return the cached payload, not
        // a fresh broker.
        let second = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "idemp-broker",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "creatorRequestId": "token-abc",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(second["brokerId"], json!(id_first));
        // Only one broker should exist in state.
        let listed = block_on(svc.handle("ListBrokers", json!({}), &ctx)).unwrap();
        assert_eq!(listed["brokerSummaries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn creator_request_id_mismatch_raises_idempotency_exception() {
        let svc = MqService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "first",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "creatorRequestId": "token-xyz",
            }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "different",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "creatorRequestId": "token-xyz",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "IdempotencyParameterMismatchException");
    }

    #[test]
    fn creator_request_id_rejects_invalid_token_shape() {
        let svc = MqService::new();
        let ctx = ctx();
        // 65-char token violates the 64-char ClientToken cap.
        let bad = "a".repeat(65);
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "bad-token",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "creatorRequestId": bad,
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn update_configuration_bumps_revision_and_validates_engine_data() {
        use base64::Engine as _;
        let svc = MqService::new();
        let ctx = ctx();

        // Create an ActiveMQ configuration. Revision starts at 1 with
        // an empty payload. UpdateConfiguration is the first call
        // that supplies bytes.
        let c = block_on(svc.handle(
            "CreateConfiguration",
            json!({
                "name": "mq-config",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
            }),
            &ctx,
        ))
        .unwrap();
        let id = c["id"].as_str().unwrap().to_string();
        assert_eq!(c["latestRevision"]["revision"], json!(1));

        // ActiveMQ payload must start with `<broker>`. Anything else
        // is rejected. The validator runs on the decoded bytes.
        let activemq_payload =
            base64::engine::general_purpose::STANDARD.encode(b"<broker xmlns=\"...\"></broker>");
        let resp = block_on(svc.handle(
            "UpdateConfiguration",
            json!({
                "configurationId": id.clone(),
                "data": activemq_payload.clone(),
                "description": "rev2",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["latestRevision"]["revision"], json!(2));
        assert_eq!(resp["latestRevision"]["description"], json!("rev2"));

        // Wrong shape (cuttlefish syntax against ActiveMQ) -> 400.
        let cuttlefish =
            base64::engine::general_purpose::STANDARD.encode(b"queue.mirroring = exactly\n");
        let err = block_on(svc.handle(
            "UpdateConfiguration",
            json!({
                "configurationId": id.clone(),
                "data": cuttlefish,
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
        assert!(
            err.message.contains("<broker"),
            "validator must explain the expected shape: {}",
            err.message
        );

        // Revision 1 + 2 should both be reachable; revision 99 should 404.
        let r1 = block_on(svc.handle(
            "DescribeConfigurationRevision",
            json!({
                "configurationId": id.clone(),
                "ConfigurationRevision": "1",
            }),
            &ctx,
        ))
        .unwrap();
        assert!(r1["data"].as_str().unwrap().is_empty());

        let r2 = block_on(svc.handle(
            "DescribeConfigurationRevision",
            json!({
                "configurationId": id.clone(),
                "ConfigurationRevision": "2",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(r2["data"], json!(activemq_payload));

        let err = block_on(svc.handle(
            "DescribeConfigurationRevision",
            json!({
                "configurationId": id,
                "ConfigurationRevision": "99",
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }

    #[test]
    fn update_configuration_rejects_xml_on_rabbitmq() {
        use base64::Engine as _;
        let svc = MqService::new();
        let ctx = ctx();

        let c = block_on(svc.handle(
            "CreateConfiguration",
            json!({
                "name": "rmq-config",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
            }),
            &ctx,
        ))
        .unwrap();
        let id = c["id"].as_str().unwrap().to_string();

        // ActiveMQ XML payload posted to a RabbitMQ configuration is
        // a hard reject.
        let xml = base64::engine::general_purpose::STANDARD.encode(b"<broker></broker>");
        let err = block_on(svc.handle(
            "UpdateConfiguration",
            json!({ "configurationId": id, "data": xml }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn describe_broker_emits_logs_summary_and_actions_required() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "logs-amq",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.m5.large",
                "logs": { "general": true, "audit": true },
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();

        let desc = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
            .unwrap();
        // LogsSummary is derived per the AWS-documented log-group
        // convention `/aws/amazonmq/{broker-id}/{general|audit}`.
        assert_eq!(desc["logs"]["general"], json!(true));
        assert_eq!(desc["logs"]["audit"], json!(true));
        assert_eq!(
            desc["logs"]["generalLogGroup"],
            json!(format!("/aws/amazonmq/{id}/general"))
        );
        assert_eq!(
            desc["logs"]["auditLogGroup"],
            json!(format!("/aws/amazonmq/{id}/audit"))
        );
        // ActionsRequired must always be present (empty when healthy)
        // so SDK clients can iterate without nil-checking.
        assert_eq!(desc["actionsRequired"], json!([]));
    }

    #[test]
    fn describe_broker_strips_audit_log_group_for_rabbitmq() {
        let svc = MqService::new();
        let ctx = ctx();
        // RabbitMQ doesn't support audit logs even when the caller
        // sets Audit=true; the summary must reflect the actual AWS
        // capability, not the request payload.
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "logs-rmq",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
                "logs": { "general": true, "audit": true },
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap();
        let desc = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id }), &ctx)).unwrap();
        assert_eq!(desc["logs"]["audit"], json!(false));
        assert!(desc["logs"].get("auditLogGroup").is_none());
    }

    #[test]
    fn describe_broker_emits_actions_required_when_no_logs() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "no-logs",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.m5.large",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap();
        let desc = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id }), &ctx)).unwrap();
        // ActionsRequired present even without a Logs config.
        assert_eq!(desc["actionsRequired"], json!([]));
        // LogsSummary derives only when Logs is configured; absent is
        // valid AWS behaviour for a broker created without logging.
        assert!(desc.get("logs").is_none());
    }

    #[test]
    fn list_brokers_paginates_with_next_token() {
        let svc = MqService::new();
        let ctx = ctx();
        // Seed enough brokers to overflow a small MaxResults window.
        for i in 0..5 {
            block_on(svc.handle(
                "CreateBroker",
                json!({
                    "brokerName": format!("pg-{i}"),
                    "engineType": "RABBITMQ",
                    "engineVersion": "3.13",
                    "hostInstanceType": "mq.t3.micro",
                }),
                &ctx,
            ))
            .unwrap();
        }

        let page1 = block_on(svc.handle("ListBrokers", json!({ "maxResults": 2 }), &ctx)).unwrap();
        assert_eq!(
            page1["brokerSummaries"].as_array().unwrap().len(),
            2,
            "first page must respect MaxResults"
        );
        let token = page1["nextToken"]
            .as_str()
            .expect("first page must hand back a NextToken when more remain");

        let page2 = block_on(svc.handle(
            "ListBrokers",
            json!({ "maxResults": 2, "nextToken": token }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page2["brokerSummaries"].as_array().unwrap().len(), 2);
        let token2 = page2["nextToken"]
            .as_str()
            .expect("second page must still hand back a NextToken");

        let page3 = block_on(svc.handle(
            "ListBrokers",
            json!({ "maxResults": 2, "nextToken": token2 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page3["brokerSummaries"].as_array().unwrap().len(), 1);
        // No more pages -> NextToken absent.
        assert!(page3.get("nextToken").is_none());
    }

    #[test]
    fn list_configurations_paginates_with_next_token() {
        let svc = MqService::new();
        let ctx = ctx();
        for i in 0..3 {
            block_on(svc.handle(
                "CreateConfiguration",
                json!({
                    "name": format!("cfg-{i}"),
                    "engineType": "RABBITMQ",
                    "engineVersion": "3.13",
                }),
                &ctx,
            ))
            .unwrap();
        }
        let page1 =
            block_on(svc.handle("ListConfigurations", json!({ "maxResults": 1 }), &ctx)).unwrap();
        assert_eq!(page1["configurations"].as_array().unwrap().len(), 1);
        assert!(page1["nextToken"].as_str().is_some());
    }

    #[test]
    fn snapshot_round_trips_pending_broker_changes() {
        // Seed a broker, stage UpdateBroker, snapshot, restore into a
        // fresh service, and assert the pending mirror survived.
        let seed = MqService::new();
        let ctx = ctx();
        let r = block_on(seed.handle(
            "CreateBroker",
            json!({
                "brokerName": "snap-broker",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.t3.micro",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();
        block_on(seed.handle(
            "UpdateBroker",
            json!({
                "brokerId": id.clone(),
                "hostInstanceType": "mq.m5.large",
                "autoMinorVersionUpgrade": false,
            }),
            &ctx,
        ))
        .unwrap();

        let bytes = seed.snapshot().expect("snapshot must succeed");
        let target = MqService::new();
        target.restore(&bytes).expect("restore must succeed");

        let desc =
            block_on(target.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
                .unwrap();
        // Pending mirror survived the round trip.
        assert_eq!(desc["pendingHostInstanceType"], json!("mq.m5.large"));
        assert_eq!(desc["pendingAutoMinorVersionUpgrade"], json!(false));
        // Live config unchanged. The reboot hasn't fired yet on the
        // restored instance either.
        assert_eq!(desc["hostInstanceType"], json!("mq.t3.micro"));
    }

    #[test]
    fn snapshot_round_trips_configuration_revisions() {
        use base64::Engine as _;
        let seed = MqService::new();
        let ctx = ctx();
        let c = block_on(seed.handle(
            "CreateConfiguration",
            json!({
                "name": "rmq-cfg",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
            }),
            &ctx,
        ))
        .unwrap();
        let id = c["id"].as_str().unwrap().to_string();
        let payload =
            base64::engine::general_purpose::STANDARD.encode(b"queue.mirroring = exactly\n");
        block_on(seed.handle(
            "UpdateConfiguration",
            json!({
                "configurationId": id.clone(),
                "data": payload.clone(),
                "description": "rev2",
            }),
            &ctx,
        ))
        .unwrap();

        let bytes = seed.snapshot().expect("snapshot must succeed");
        let target = MqService::new();
        target.restore(&bytes).expect("restore must succeed");

        let rev2 = block_on(target.handle(
            "DescribeConfigurationRevision",
            json!({
                "configurationId": id,
                "ConfigurationRevision": "2",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(rev2["data"], json!(payload));
        assert_eq!(rev2["description"], json!("rev2"));
    }

    #[test]
    fn update_broker_stages_pending_until_reboot() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "pending-broker",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.t3.micro",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();

        block_on(svc.handle(
            "UpdateBroker",
            json!({
                "brokerId": id.clone(),
                "hostInstanceType": "mq.m5.large",
                "autoMinorVersionUpgrade": false,
            }),
            &ctx,
        ))
        .unwrap();

        // Live config unchanged before reboot; pending mirror reflects
        // the staged diff via Pending* fields.
        let pre = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
            .unwrap();
        assert_eq!(pre["hostInstanceType"], json!("mq.t3.micro"));
        assert_eq!(pre["autoMinorVersionUpgrade"], json!(true));
        assert_eq!(pre["pendingHostInstanceType"], json!("mq.m5.large"));
        assert_eq!(pre["pendingAutoMinorVersionUpgrade"], json!(false));

        // Reboot promotes the staged values and clears Pending*.
        block_on(svc.handle("RebootBroker", json!({ "brokerId": id.clone() }), &ctx)).unwrap();
        let post = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
            .unwrap();
        assert_eq!(post["hostInstanceType"], json!("mq.m5.large"));
        assert_eq!(post["autoMinorVersionUpgrade"], json!(false));
        assert!(post.get("pendingHostInstanceType").is_none());
        assert!(post.get("pendingAutoMinorVersionUpgrade").is_none());
    }

    #[test]
    fn update_broker_stages_logs_and_configuration_for_reboot() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "log-rotate",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.m5.large",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();

        block_on(svc.handle(
            "UpdateBroker",
            json!({
                "brokerId": id.clone(),
                "logs": { "general": true, "audit": true },
                "configuration": { "id": "c-new", "revision": 2 },
            }),
            &ctx,
        ))
        .unwrap();
        let pre = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
            .unwrap();
        // No Logs yet on live config. Pending only.
        assert!(pre.get("logs").is_none());
        assert_eq!(pre["pendingLogs"]["audit"], json!(true));
        assert_eq!(pre["pendingConfiguration"]["id"], json!("c-new"));

        block_on(svc.handle("RebootBroker", json!({ "brokerId": id.clone() }), &ctx)).unwrap();
        let post = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id.clone() }), &ctx))
            .unwrap();
        // `logs` is the summary derived from the now-live config.
        assert_eq!(post["logs"]["audit"], json!(true));
        assert_eq!(post["configurations"]["current"]["id"], json!("c-new"));
        assert!(post.get("pendingLogs").is_none());
        assert!(post.get("pendingConfiguration").is_none());
    }

    #[test]
    fn create_broker_persists_full_config_surface() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "full-cfg",
                "engineType": "ACTIVEMQ",
                "engineVersion": "5.18",
                "hostInstanceType": "mq.m5.large",
                "deploymentMode": "SINGLE_INSTANCE",
                "encryptionOptions": {
                    "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
                    "useAwsOwnedKey": false,
                },
                "logs": { "general": true, "audit": true },
                "maintenanceWindowStartTime": {
                    "dayOfWeek": "SUNDAY",
                    "timeOfDay": "05:00",
                    "timeZone": "UTC",
                },
                "configuration": { "id": "c-abc", "revision": 1 },
                "dataReplicationMode": "NONE",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["brokerId"].as_str().unwrap().to_string();

        let desc = block_on(svc.handle("DescribeBroker", json!({ "brokerId": id }), &ctx)).unwrap();
        assert_eq!(
            desc["encryptionOptions"]["kmsKeyId"],
            json!("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
        assert_eq!(desc["logs"]["audit"], json!(true));
        assert_eq!(
            desc["maintenanceWindowStartTime"]["dayOfWeek"],
            json!("SUNDAY")
        );
        assert_eq!(desc["configurations"]["current"]["id"], json!("c-abc"));
        assert_eq!(desc["dataReplicationMode"], json!("NONE"));
    }

    #[test]
    fn tags_round_trip_create_list_delete() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "tagged",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.t3.micro",
            }),
            &ctx,
        ))
        .unwrap();
        let arn = r["brokerArn"].as_str().unwrap().to_string();

        // CreateTags merges (upsert) into the broker's tag map.
        block_on(svc.handle(
            "CreateTags",
            json!({ "resourceArn": arn, "tags": { "Owner": "alice", "Cost": "eng" } }),
            &ctx,
        ))
        .unwrap();
        let listed = block_on(svc.handle("ListTags", json!({ "resourceArn": arn }), &ctx)).unwrap();
        assert_eq!(listed["tags"]["Owner"], json!("alice"));
        assert_eq!(listed["tags"]["Cost"], json!("eng"));

        // DeleteTags removes only the named keys.
        block_on(svc.handle(
            "DeleteTags",
            json!({ "resourceArn": arn, "tagKeys": ["Cost"] }),
            &ctx,
        ))
        .unwrap();
        let after = block_on(svc.handle("ListTags", json!({ "resourceArn": arn }), &ctx)).unwrap();
        assert_eq!(after["tags"]["Owner"], json!("alice"));
        assert!(after["tags"].get("Cost").is_none());
    }

    #[test]
    fn create_tags_rejects_reserved_aws_prefix() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "brokerName": "reserved-tags",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
                "hostInstanceType": "mq.t3.micro",
            }),
            &ctx,
        ))
        .unwrap();
        let arn = r["brokerArn"].as_str().unwrap().to_string();
        let err = block_on(svc.handle(
            "CreateTags",
            json!({ "resourceArn": arn, "tags": { "aws:internal": "x" } }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn list_tags_on_unknown_arn_is_not_found() {
        let svc = MqService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "ListTags",
            json!({ "resourceArn": "arn:aws:mq:us-east-1:000000000000:broker:b-missing" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "NotFoundException");
    }

    #[test]
    fn configuration_arn_is_taggable() {
        let svc = MqService::new();
        let ctx = ctx();
        let c = block_on(svc.handle(
            "CreateConfiguration",
            json!({
                "name": "tag-cfg",
                "engineType": "RABBITMQ",
                "engineVersion": "3.13",
            }),
            &ctx,
        ))
        .unwrap();
        let arn = c["arn"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CreateTags",
            json!({ "resourceArn": arn, "tags": { "Team": "platform" } }),
            &ctx,
        ))
        .unwrap();
        let listed = block_on(svc.handle("ListTags", json!({ "resourceArn": arn }), &ctx)).unwrap();
        assert_eq!(listed["tags"]["Team"], json!("platform"));
    }
}
