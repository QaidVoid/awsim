pub mod auth;
pub mod error;
pub mod events;
pub mod gateway;
pub mod persistence;
pub mod protocol;
pub mod router;
pub mod state;

pub use error::AwsError;
pub use events::{EventBus, InternalEvent};
pub use gateway::AppState;
pub use persistence::PersistenceManager;
pub use protocol::{Protocol, RouteDefinition};
pub use router::RequestContext;
pub use state::AccountRegionStore;

use serde_json::Value;

/// Trait that every AWS service crate must implement.
///
/// Each service (S3, SQS, DynamoDB, etc.) implements this trait in its own crate.
/// The main `awsim` binary registers all service handlers with the gateway router.
#[async_trait::async_trait]
pub trait ServiceHandler: Send + Sync {
    /// The AWS service name (e.g., "s3", "sqs", "dynamodb").
    fn service_name(&self) -> &str;

    /// The signing name used in SigV4 Authorization headers.
    /// Usually the same as service_name, but not always.
    fn signing_name(&self) -> &str {
        self.service_name()
    }

    /// The primary protocol this service uses.
    fn protocol(&self) -> Protocol;

    /// Route definitions for REST-protocol services.
    /// Not needed for RPC-style protocols (awsJson, awsQuery).
    fn routes(&self) -> Vec<RouteDefinition> {
        Vec::new()
    }

    /// Handle an AWS API operation.
    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError>;

    /// Serialize the service's state to bytes for persistence.
    ///
    /// Return `None` if this service does not support snapshots.
    fn snapshot(&self) -> Option<Vec<u8>> {
        None
    }

    /// Restore the service's state from a previous snapshot.
    ///
    /// The default implementation is a no-op and always succeeds.
    fn restore(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}
