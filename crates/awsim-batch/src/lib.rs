mod operations;
mod state;

pub use state::BatchState;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct BatchService {
    store: AccountRegionStore<BatchState>,
}

impl BatchService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for BatchService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for BatchService {
    fn service_name(&self) -> &str {
        "batch"
    }

    fn signing_name(&self) -> &str {
        "batch"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/createcomputeenvironment",
                operation: "CreateComputeEnvironment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/describecomputeenvironments",
                operation: "DescribeComputeEnvironments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/updatecomputeenvironment",
                operation: "UpdateComputeEnvironment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/deletecomputeenvironment",
                operation: "DeleteComputeEnvironment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/createjobqueue",
                operation: "CreateJobQueue",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/describejobqueues",
                operation: "DescribeJobQueues",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/updatejobqueue",
                operation: "UpdateJobQueue",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/deletejobqueue",
                operation: "DeleteJobQueue",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/registerjobdefinition",
                operation: "RegisterJobDefinition",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/describejobdefinitions",
                operation: "DescribeJobDefinitions",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/deregisterjobdefinition",
                operation: "DeregisterJobDefinition",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/submitjob",
                operation: "SubmitJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/describejobs",
                operation: "DescribeJobs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/listjobs",
                operation: "ListJobs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/terminatejob",
                operation: "TerminateJob",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/canceljob",
                operation: "CancelJob",
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
        debug!(operation, "Batch request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        match operation {
            "CreateComputeEnvironment" => operations::compute::create_compute_environment(&state, &input, ctx),
            "DescribeComputeEnvironments" => operations::compute::describe_compute_environments(&state, &input, ctx),
            "UpdateComputeEnvironment" => operations::compute::update_compute_environment(&state, &input, ctx),
            "DeleteComputeEnvironment" => operations::compute::delete_compute_environment(&state, &input, ctx),
            "CreateJobQueue" => operations::queues::create_job_queue(&state, &input, ctx),
            "DescribeJobQueues" => operations::queues::describe_job_queues(&state, &input, ctx),
            "UpdateJobQueue" => operations::queues::update_job_queue(&state, &input, ctx),
            "DeleteJobQueue" => operations::queues::delete_job_queue(&state, &input, ctx),
            "RegisterJobDefinition" => operations::jobs::register_job_definition(&state, &input, ctx),
            "DescribeJobDefinitions" => operations::jobs::describe_job_definitions(&state, &input, ctx),
            "DeregisterJobDefinition" => operations::jobs::deregister_job_definition(&state, &input, ctx),
            "SubmitJob" => operations::jobs::submit_job(&state, &input, ctx),
            "DescribeJobs" => operations::jobs::describe_jobs(&state, &input, ctx),
            "ListJobs" => operations::jobs::list_jobs(&state, &input, ctx),
            "TerminateJob" => operations::jobs::terminate_job(&state, &input, ctx),
            "CancelJob" => operations::jobs::cancel_job(&state, &input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
