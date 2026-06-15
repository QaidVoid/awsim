//! Amazon RDS Data API emulator for AWSim.
//!
//! Implements the Aurora HTTP SQL endpoint (`rds-data`): `ExecuteStatement`,
//! `BatchExecuteStatement`, and the `BeginTransaction` / `CommitTransaction`
//! / `RollbackTransaction` lifecycle. Statements execute against a real
//! PostgreSQL started on demand in a Docker container, one per cluster,
//! so SQL runs with genuine PostgreSQL semantics rather than a simulated
//! dialect.
//!
//! This crate is compiled only when the binary's `rds-data` feature is
//! enabled, since it depends on Docker being available at runtime.
//!
//! Configuration (environment variables):
//! - `AWSIM_RDS_DATA_PG_IMAGE`: the PostgreSQL image to run. Any
//!   `postgres:NN` tag works (14 through 18), since the Data API only
//!   uses wire-protocol features stable across those releases. Defaults
//!   to `postgres:16-alpine`.
//! - `AWSIM_RDS_DATA_PG_HOST`: the host AWSim connects to in order to
//!   reach the container's published port. Defaults to `127.0.0.1` for
//!   the on-host case; set it (for example to `host.docker.internal`)
//!   when AWSim itself runs in a container against the host's Docker
//!   socket.

mod engine;
mod operations;
mod types;

use async_trait::async_trait;
use awsim_core::{AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use engine::PgEngine;

/// Default PostgreSQL image used for the per-cluster containers. Override
/// with the `AWSIM_RDS_DATA_PG_IMAGE` environment variable.
const DEFAULT_PG_IMAGE: &str = "postgres:16-alpine";

/// The AWSim RDS Data API service handler.
pub struct RdsDataService {
    engine: PgEngine,
}

impl RdsDataService {
    pub fn new() -> Self {
        let image = std::env::var("AWSIM_RDS_DATA_PG_IMAGE")
            .unwrap_or_else(|_| DEFAULT_PG_IMAGE.to_string());
        let host =
            std::env::var("AWSIM_RDS_DATA_PG_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        Self {
            engine: PgEngine::new(image, host),
        }
    }
}

impl Default for RdsDataService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for RdsDataService {
    fn service_name(&self) -> &str {
        "rds-data"
    }

    fn signing_name(&self) -> &str {
        "rds-data"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/Execute",
                operation: "ExecuteStatement",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/BatchExecute",
                operation: "BatchExecuteStatement",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/BeginTransaction",
                operation: "BeginTransaction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/CommitTransaction",
                operation: "CommitTransaction",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/RollbackTransaction",
                operation: "RollbackTransaction",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        _ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "RDS Data API request");
        match operation {
            "ExecuteStatement" => operations::execute_statement(&self.engine, &input).await,
            "BatchExecuteStatement" => {
                operations::batch_execute_statement(&self.engine, &input).await
            }
            "BeginTransaction" => operations::begin_transaction(&self.engine, &input).await,
            "CommitTransaction" => operations::commit_transaction(&self.engine, &input).await,
            "RollbackTransaction" => operations::rollback_transaction(&self.engine, &input).await,
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
