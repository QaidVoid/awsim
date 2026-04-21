pub mod error;
pub mod protocol;
pub mod router;
pub mod state;

pub use error::AwsError;
pub use protocol::Protocol;
pub use router::RequestContext;

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
    /// Usually the same as service_name, but not always (e.g., "execute-api" for API Gateway).
    fn signing_name(&self) -> &str {
        self.service_name()
    }

    /// The primary protocol this service uses.
    fn protocol(&self) -> Protocol;

    /// Handle an AWS API operation.
    ///
    /// - `operation`: The operation name (e.g., "CreateBucket", "PutItem")
    /// - `input`: The parsed request body as a JSON Value
    /// - `ctx`: Request context (account ID, region, request ID, etc.)
    ///
    /// Returns the response body as a JSON Value, or an AwsError.
    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError>;
}
