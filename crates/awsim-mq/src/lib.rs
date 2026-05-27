//! Amazon MQ emulator. Brokers, broker users, and configuration metadata.
//! Emulator never spins up real ActiveMQ/RabbitMQ — broker state is always
//! `RUNNING` immediately after CreateBroker.

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

    /// Count active brokers for a given account+region — used by the
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
                path_pattern: "/v1/brokers/{BrokerId}",
                operation: "DescribeBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/brokers/{BrokerId}",
                operation: "DeleteBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/brokers/{BrokerId}",
                operation: "UpdateBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/brokers/{BrokerId}/reboot",
                operation: "RebootBroker",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/brokers/{BrokerId}/users/{Username}",
                operation: "CreateUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers/{BrokerId}/users/{Username}",
                operation: "DescribeUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/brokers/{BrokerId}/users",
                operation: "ListUsers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/brokers/{BrokerId}/users/{Username}",
                operation: "DeleteUser",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/brokers/{BrokerId}/users/{Username}",
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
                path_pattern: "/v1/configurations/{ConfigurationId}",
                operation: "DescribeConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/configurations/{ConfigurationId}",
                operation: "UpdateConfiguration",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/configurations/{ConfigurationId}/revisions/{ConfigurationRevision}",
                operation: "DescribeConfigurationRevision",
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
            _ => Err(AwsError::unknown_operation(operation)),
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
                "BrokerName": "primary",
                "EngineType": "RABBITMQ",
                "EngineVersion": "3.13",
                "HostInstanceType": "mq.m5.large",
                "DeploymentMode": "SINGLE_INSTANCE",
                "Users": [{ "Username": "admin", "ConsoleAccess": true }]
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["BrokerId"].as_str().unwrap().to_string();

        let described =
            block_on(svc.handle("DescribeBroker", json!({ "BrokerId": id }), &ctx)).unwrap();
        assert_eq!(described["BrokerState"], "RUNNING");
        assert_eq!(described["Users"].as_array().unwrap().len(), 1);
        assert_eq!(described["EngineType"], "RABBITMQ");

        block_on(svc.handle(
            "CreateUser",
            json!({ "BrokerId": id, "Username": "app", "Password": "x" }),
            &ctx,
        ))
        .unwrap();
        let users = block_on(svc.handle("ListUsers", json!({ "BrokerId": id }), &ctx)).unwrap();
        assert_eq!(users["Users"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn create_broker_rejects_invalid_name() {
        let svc = MqService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateBroker",
            json!({
                "BrokerName": "bad name!",
                "EngineType": "ACTIVEMQ",
                "EngineVersion": "5.18",
                "HostInstanceType": "mq.t3.micro"
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
                "BrokerName": "rmq",
                "EngineType": "RABBITMQ",
                "EngineVersion": "3.13",
                "HostInstanceType": "mq.m5.large",
                "StorageType": "EFS"
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
                "BrokerName": "ldap-broker",
                "EngineType": "ACTIVEMQ",
                "EngineVersion": "5.18",
                "HostInstanceType": "mq.t3.micro",
                "AuthenticationStrategy": "LDAP"
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
            "BrokerName": "dup",
            "EngineType": "ACTIVEMQ",
            "EngineVersion": "5.18",
            "HostInstanceType": "mq.t3.micro"
        });
        block_on(svc.handle("CreateBroker", body.clone(), &ctx)).unwrap();
        let err = block_on(svc.handle("CreateBroker", body, &ctx)).unwrap_err();
        assert_eq!(err.code, "ConflictException");
    }

    fn make_broker_with_user(svc: &MqService, ctx: &RequestContext) -> String {
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "BrokerName": "secrets",
                "EngineType": "ACTIVEMQ",
                "EngineVersion": "5.18",
                "HostInstanceType": "mq.t3.micro",
                "Users": [{
                    "Username": "alice",
                    "ConsoleAccess": false,
                    "Groups": ["g1"],
                    "Password": "hunter2"
                }]
            }),
            ctx,
        ))
        .unwrap();
        r["BrokerId"].as_str().unwrap().to_string()
    }

    #[test]
    fn describe_user_does_not_surface_password() {
        let svc = MqService::new();
        let ctx = ctx();
        let id = make_broker_with_user(&svc, &ctx);
        let desc = block_on(svc.handle(
            "DescribeUser",
            json!({ "BrokerId": id, "Username": "alice" }),
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
                "BrokerId": id,
                "Username": "alice",
                "ConsoleAccess": true,
                "Groups": ["admins"],
            }),
            &ctx,
        ))
        .unwrap();
        let desc = block_on(svc.handle(
            "DescribeUser",
            json!({ "BrokerId": id, "Username": "alice" }),
            &ctx,
        ))
        .unwrap();
        // Live values unchanged.
        assert_eq!(desc["ConsoleAccess"], false);
        assert_eq!(desc["Groups"][0], "g1");
        // Pending mirror reflects the requested update.
        let pending = desc["Pending"].as_object().expect("Pending populated");
        assert_eq!(pending["ConsoleAccess"], true);
        assert_eq!(pending["Groups"][0], "admins");
    }

    #[test]
    fn update_configuration_bumps_revision_and_validates_engine_data() {
        use base64::Engine as _;
        let svc = MqService::new();
        let ctx = ctx();

        // Create an ActiveMQ configuration. Revision starts at 1 with
        // an empty payload — UpdateConfiguration is the first call
        // that supplies bytes.
        let c = block_on(svc.handle(
            "CreateConfiguration",
            json!({
                "Name": "mq-config",
                "EngineType": "ACTIVEMQ",
                "EngineVersion": "5.18",
            }),
            &ctx,
        ))
        .unwrap();
        let id = c["Id"].as_str().unwrap().to_string();
        assert_eq!(c["LatestRevision"]["Revision"], json!(1));

        // ActiveMQ payload must start with `<broker>` — anything else
        // is rejected. The validator runs on the decoded bytes.
        let activemq_payload =
            base64::engine::general_purpose::STANDARD.encode(b"<broker xmlns=\"...\"></broker>");
        let resp = block_on(svc.handle(
            "UpdateConfiguration",
            json!({
                "ConfigurationId": id.clone(),
                "Data": activemq_payload.clone(),
                "Description": "rev2",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["LatestRevision"]["Revision"], json!(2));
        assert_eq!(resp["LatestRevision"]["Description"], json!("rev2"));

        // Wrong shape (cuttlefish syntax against ActiveMQ) -> 400.
        let cuttlefish =
            base64::engine::general_purpose::STANDARD.encode(b"queue.mirroring = exactly\n");
        let err = block_on(svc.handle(
            "UpdateConfiguration",
            json!({
                "ConfigurationId": id.clone(),
                "Data": cuttlefish,
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
                "ConfigurationId": id.clone(),
                "ConfigurationRevision": "1",
            }),
            &ctx,
        ))
        .unwrap();
        assert!(r1["Data"].as_str().unwrap().is_empty());

        let r2 = block_on(svc.handle(
            "DescribeConfigurationRevision",
            json!({
                "ConfigurationId": id.clone(),
                "ConfigurationRevision": "2",
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(r2["Data"], json!(activemq_payload));

        let err = block_on(svc.handle(
            "DescribeConfigurationRevision",
            json!({
                "ConfigurationId": id,
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
                "Name": "rmq-config",
                "EngineType": "RABBITMQ",
                "EngineVersion": "3.13",
            }),
            &ctx,
        ))
        .unwrap();
        let id = c["Id"].as_str().unwrap().to_string();

        // ActiveMQ XML payload posted to a RabbitMQ configuration is
        // a hard reject.
        let xml = base64::engine::general_purpose::STANDARD.encode(b"<broker></broker>");
        let err = block_on(svc.handle(
            "UpdateConfiguration",
            json!({ "ConfigurationId": id, "Data": xml }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "BadRequestException");
    }

    #[test]
    fn create_broker_persists_full_config_surface() {
        let svc = MqService::new();
        let ctx = ctx();
        let r = block_on(svc.handle(
            "CreateBroker",
            json!({
                "BrokerName": "full-cfg",
                "EngineType": "ACTIVEMQ",
                "EngineVersion": "5.18",
                "HostInstanceType": "mq.m5.large",
                "DeploymentMode": "SINGLE_INSTANCE",
                "EncryptionOptions": {
                    "KmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc",
                    "UseAwsOwnedKey": false,
                },
                "Logs": { "General": true, "Audit": true },
                "MaintenanceWindowStartTime": {
                    "DayOfWeek": "SUNDAY",
                    "TimeOfDay": "05:00",
                    "TimeZone": "UTC",
                },
                "Configuration": { "Id": "c-abc", "Revision": 1 },
                "DataReplicationMode": "NONE",
            }),
            &ctx,
        ))
        .unwrap();
        let id = r["BrokerId"].as_str().unwrap().to_string();

        let desc = block_on(svc.handle("DescribeBroker", json!({ "BrokerId": id }), &ctx)).unwrap();
        assert_eq!(
            desc["EncryptionOptions"]["KmsKeyId"],
            json!("arn:aws:kms:us-east-1:000000000000:key/abc")
        );
        assert_eq!(desc["Logs"]["Audit"], json!(true));
        assert_eq!(
            desc["MaintenanceWindowStartTime"]["DayOfWeek"],
            json!("SUNDAY")
        );
        assert_eq!(desc["Configurations"]["Current"]["Id"], json!("c-abc"));
        assert_eq!(desc["DataReplicationMode"], json!("NONE"));
    }
}
