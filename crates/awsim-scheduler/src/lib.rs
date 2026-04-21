mod operations;
mod state;

pub use state::SchedulerState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::SchedulerStateSnapshot;

/// The EventBridge Scheduler service handler.
pub struct SchedulerService {
    store: AccountRegionStore<SchedulerState>,
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<SchedulerState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for SchedulerService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for SchedulerService {
    fn service_name(&self) -> &str {
        "scheduler"
    }

    fn signing_name(&self) -> &str {
        "scheduler"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/schedules/{Name}",
                operation: "CreateSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedules/{Name}",
                operation: "GetSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedules",
                operation: "ListSchedules",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/schedules/{Name}",
                operation: "DeleteSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/schedules/{Name}",
                operation: "UpdateSchedule",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/schedule-groups/{Name}",
                operation: "CreateScheduleGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedule-groups/{Name}",
                operation: "GetScheduleGroup",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/schedule-groups",
                operation: "ListScheduleGroups",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/schedule-groups/{Name}",
                operation: "DeleteScheduleGroup",
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
        debug!(operation, "EventBridge Scheduler request");
        let state = self.get_state(ctx);

        match operation {
            // Schedule operations
            "CreateSchedule" => operations::schedules::create_schedule(&state, &input, ctx),
            "GetSchedule" => operations::schedules::get_schedule(&state, &input, ctx),
            "ListSchedules" => operations::schedules::list_schedules(&state, &input, ctx),
            "DeleteSchedule" => operations::schedules::delete_schedule(&state, &input, ctx),
            "UpdateSchedule" => operations::schedules::update_schedule(&state, &input, ctx),

            // Schedule Group operations
            "CreateScheduleGroup" => {
                operations::groups::create_schedule_group(&state, &input, ctx)
            }
            "GetScheduleGroup" => operations::groups::get_schedule_group(&state, &input, ctx),
            "ListScheduleGroups" => {
                operations::groups::list_schedule_groups(&state, &input, ctx)
            }
            "DeleteScheduleGroup" => {
                operations::groups::delete_schedule_group(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut snap = SchedulerStateSnapshot {
            schedules: vec![],
            schedule_groups: vec![],
        };

        for (_, state) in self.store.iter_all() {
            let s = state.to_snapshot();
            snap.schedules.extend(s.schedules);
            snap.schedule_groups.extend(s.schedule_groups);
        }

        serde_json::to_vec(&snap).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: SchedulerStateSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;

        let state = self.store.get("000000000000", "us-east-1");
        state.restore_from_snapshot(snapshot);

        Ok(())
    }
}
