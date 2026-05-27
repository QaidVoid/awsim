//! AWS AppConfig (control plane) and AppConfigData (data plane) emulator.
//!
//! Both services share the same backing state. AppConfig handles the
//! applications/environments/profiles/versions/deployments CRUD; AppConfigData
//! handles `StartConfigurationSession` / `GetLatestConfiguration` for runtime
//! config polling.

mod operations;
pub mod state;

pub use state::AppConfigState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct AppConfigService {
    store: AccountRegionStore<AppConfigState>,
}

impl AppConfigService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<AppConfigState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<AppConfigState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for AppConfigService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for AppConfigService {
    fn service_name(&self) -> &str {
        "appconfig"
    }

    fn signing_name(&self) -> &str {
        "appconfig"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Applications
            RouteDefinition {
                method: "POST",
                path_pattern: "/applications",
                operation: "CreateApplication",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications",
                operation: "ListApplications",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}",
                operation: "GetApplication",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/applications/{ApplicationId}",
                operation: "UpdateApplication",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/applications/{ApplicationId}",
                operation: "DeleteApplication",
                required_query_param: None,
            },
            // Environments
            RouteDefinition {
                method: "POST",
                path_pattern: "/applications/{ApplicationId}/environments",
                operation: "CreateEnvironment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/environments",
                operation: "ListEnvironments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/environments/{EnvironmentId}",
                operation: "GetEnvironment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/applications/{ApplicationId}/environments/{EnvironmentId}",
                operation: "DeleteEnvironment",
                required_query_param: None,
            },
            // Configuration profiles
            RouteDefinition {
                method: "POST",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles",
                operation: "CreateConfigurationProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles",
                operation: "ListConfigurationProfiles",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles/{ConfigurationProfileId}",
                operation: "GetConfigurationProfile",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles/{ConfigurationProfileId}",
                operation: "DeleteConfigurationProfile",
                required_query_param: None,
            },
            // Hosted versions
            RouteDefinition {
                method: "POST",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles/{ConfigurationProfileId}/hostedconfigurationversions",
                operation: "CreateHostedConfigurationVersion",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/configurationprofiles/{ConfigurationProfileId}/hostedconfigurationversions/{VersionNumber}",
                operation: "GetHostedConfigurationVersion",
                required_query_param: None,
            },
            // Deployments
            RouteDefinition {
                method: "POST",
                path_pattern: "/applications/{ApplicationId}/environments/{EnvironmentId}/deployments",
                operation: "StartDeployment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/environments/{EnvironmentId}/deployments",
                operation: "ListDeployments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/applications/{ApplicationId}/environments/{EnvironmentId}/deployments/{DeploymentNumber}",
                operation: "GetDeployment",
                required_query_param: None,
            },
            // Deployment strategies
            RouteDefinition {
                method: "POST",
                path_pattern: "/deploymentstrategies",
                operation: "CreateDeploymentStrategy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/deploymentstrategies",
                operation: "ListDeploymentStrategies",
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
        debug!(operation, "AppConfig request");
        let state = self.get_state(ctx);
        match operation {
            "CreateApplication" => {
                operations::applications::create_application(&state, &input, ctx)
            }
            "GetApplication" => operations::applications::get_application(&state, &input, ctx),
            "ListApplications" => operations::applications::list_applications(&state, &input, ctx),
            "UpdateApplication" => {
                operations::applications::update_application(&state, &input, ctx)
            }
            "DeleteApplication" => {
                operations::applications::delete_application(&state, &input, ctx)
            }
            "CreateEnvironment" => {
                operations::environments::create_environment(&state, &input, ctx)
            }
            "GetEnvironment" => operations::environments::get_environment(&state, &input, ctx),
            "ListEnvironments" => operations::environments::list_environments(&state, &input, ctx),
            "DeleteEnvironment" => {
                operations::environments::delete_environment(&state, &input, ctx)
            }
            "CreateConfigurationProfile" => {
                operations::profiles::create_profile(&state, &input, ctx)
            }
            "GetConfigurationProfile" => operations::profiles::get_profile(&state, &input, ctx),
            "ListConfigurationProfiles" => operations::profiles::list_profiles(&state, &input, ctx),
            "DeleteConfigurationProfile" => {
                operations::profiles::delete_profile(&state, &input, ctx)
            }
            "CreateHostedConfigurationVersion" => {
                operations::hosted::create_hosted_version(&state, &input, ctx)
            }
            "GetHostedConfigurationVersion" => {
                operations::hosted::get_hosted_version(&state, &input, ctx)
            }
            "StartDeployment" => operations::deployments::start_deployment(&state, &input, ctx),
            "GetDeployment" => operations::deployments::get_deployment(&state, &input, ctx),
            "ListDeployments" => operations::deployments::list_deployments(&state, &input, ctx),
            "CreateDeploymentStrategy" => {
                operations::strategies::create_strategy(&state, &input, ctx)
            }
            "ListDeploymentStrategies" => {
                operations::strategies::list_strategies(&state, &input, ctx)
            }
            "GetDeploymentStrategy" => operations::strategies::get_strategy(&state, &input, ctx),
            "UpdateDeploymentStrategy" => {
                operations::strategies::update_strategy(&state, &input, ctx)
            }
            "DeleteDeploymentStrategy" => {
                operations::strategies::delete_strategy(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = empty_snapshot();
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.applications.extend(s.applications);
            all.environments.extend(s.environments);
            all.profiles.extend(s.profiles);
            all.hosted_versions.extend(s.hosted_versions);
            all.deployments.extend(s.deployments);
            all.deployment_strategies.extend(s.deployment_strategies);
            all.sessions.extend(s.sessions);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: state::AppConfigSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

/// AppConfigData (data plane) shares the AppConfig store. This service is what
/// runtime workloads hit to fetch the latest configuration without going
/// through the control plane.
pub struct AppConfigDataService {
    store: AccountRegionStore<AppConfigState>,
}

impl AppConfigDataService {
    pub fn new(store: AccountRegionStore<AppConfigState>) -> Self {
        Self { store }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<AppConfigState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

#[async_trait]
impl ServiceHandler for AppConfigDataService {
    fn service_name(&self) -> &str {
        "appconfigdata"
    }

    fn signing_name(&self) -> &str {
        "appconfig"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/configurationsessions",
                operation: "StartConfigurationSession",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/configuration",
                operation: "GetLatestConfiguration",
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
        debug!(operation, "AppConfigData request");
        let state = self.get_state(ctx);
        match operation {
            "StartConfigurationSession" => {
                operations::data::start_configuration_session(&state, &input, ctx)
            }
            "GetLatestConfiguration" => {
                operations::data::get_latest_configuration(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

fn empty_snapshot() -> state::AppConfigSnapshot {
    state::AppConfigSnapshot {
        applications: vec![],
        environments: vec![],
        profiles: vec![],
        hosted_versions: vec![],
        deployments: vec![],
        deployment_strategies: vec![],
        sessions: Default::default(),
    }
}

/// Single registration point for AppConfig.
///
/// The control plane (`AppConfigService`) and the AppConfigData data
/// plane both sign as `appconfig`. The gateway keys handlers and routes
/// by signing name, so registering them separately makes the second
/// clobber the first (the data plane was shadowing the control plane,
/// so `GET /applications` returned "Unknown operation"). This facade
/// owns both - sharing one store - and dispatches by operation.
pub struct AppConfig {
    control: AppConfigService,
    data: AppConfigDataService,
}

impl AppConfig {
    pub fn new() -> Self {
        let control = AppConfigService::new();
        let data = AppConfigDataService::new(control.store());
        Self { control, data }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for AppConfig {
    fn service_name(&self) -> &str {
        "appconfig"
    }

    fn signing_name(&self) -> &str {
        "appconfig"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        let mut routes = self.control.routes();
        routes.extend(self.data.routes());
        routes
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match operation {
            "StartConfigurationSession" | "GetLatestConfiguration" => {
                self.data.handle(operation, input, ctx).await
            }
            _ => self.control.handle(operation, input, ctx).await,
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        self.control.snapshot()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        self.control.restore(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("appconfig", "us-east-1")
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
    fn full_flow_to_get_latest_configuration() {
        let svc = AppConfigService::new();
        let data_svc = AppConfigDataService::new(svc.store());
        let ctx = ctx();

        // App
        let a =
            block_on(svc.handle("CreateApplication", json!({ "Name": "checkout" }), &ctx)).unwrap();
        let app_id = a["Id"].as_str().unwrap().to_string();

        // Env
        let e = block_on(svc.handle(
            "CreateEnvironment",
            json!({ "ApplicationId": app_id, "Name": "prod" }),
            &ctx,
        ))
        .unwrap();
        let env_id = e["Id"].as_str().unwrap().to_string();

        // Profile
        let p = block_on(svc.handle(
            "CreateConfigurationProfile",
            json!({ "ApplicationId": app_id, "Name": "feature-flags", "LocationUri": "hosted" }),
            &ctx,
        ))
        .unwrap();
        let pid = p["Id"].as_str().unwrap().to_string();

        // Hosted version
        let body = b"{\"new_checkout\": true}";
        block_on(svc.handle(
            "CreateHostedConfigurationVersion",
            json!({
                "ApplicationId": app_id,
                "ConfigurationProfileId": pid,
                "Content": B64.encode(body),
                "ContentType": "application/json"
            }),
            &ctx,
        ))
        .unwrap();

        // Deployment
        let dep = block_on(svc.handle(
            "StartDeployment",
            json!({
                "ApplicationId": app_id,
                "EnvironmentId": env_id,
                "ConfigurationProfileId": pid,
                "DeploymentStrategyId": "AppConfig.AllAtOnce",
                "ConfigurationVersion": "1"
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(dep["State"], "COMPLETE");

        // Data plane: start session by name, fetch latest by token.
        let session = block_on(data_svc.handle(
            "StartConfigurationSession",
            json!({
                "ApplicationIdentifier": "checkout",
                "EnvironmentIdentifier": "prod",
                "ConfigurationProfileIdentifier": "feature-flags"
            }),
            &ctx,
        ))
        .unwrap();
        let token = session["InitialConfigurationToken"]
            .as_str()
            .unwrap()
            .to_string();
        let latest = block_on(data_svc.handle(
            "GetLatestConfiguration",
            json!({ "ConfigurationToken": token }),
            &ctx,
        ))
        .unwrap();
        let content = B64
            .decode(latest["Configuration"].as_str().unwrap())
            .unwrap();
        assert_eq!(content, body);
        // NextPollConfigurationToken is rotated.
        assert!(latest["NextPollConfigurationToken"].as_str().is_some());
    }

    #[test]
    fn predefined_strategies_appear_on_list() {
        let svc = AppConfigService::new();
        let ctx = ctx();
        let r = block_on(svc.handle("ListDeploymentStrategies", json!({}), &ctx)).unwrap();
        let names: Vec<&str> = r["Items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["Name"].as_str())
            .collect();
        assert!(names.iter().any(|n| n == &"AppConfig.AllAtOnce"));
    }
}
