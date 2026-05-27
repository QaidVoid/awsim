use axum::http::StatusCode;
use serde::Serialize;
use serde_json::{Map, Value};

/// Represents an AWS API error response.
#[derive(Debug, Clone, Serialize)]
pub struct AwsError {
    /// HTTP status code (e.g., 404, 400, 500)
    #[serde(skip)]
    pub status: StatusCode,

    /// AWS error code (e.g., "NoSuchBucket", "ResourceNotFoundException")
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Error type: "Sender" (client error) or "Receiver" (server error)
    pub error_type: ErrorType,

    /// Extra JSON fields merged into the serialized error body.
    ///
    /// Some AWS exceptions carry structured data alongside the standard
    /// `__type` / `message` envelope — for example, DynamoDB's
    /// `TransactionCanceledException` includes a `CancellationReasons` array,
    /// and `ConditionalCheckFailedException` may include the existing `Item`.
    /// Use [`Self::with_extras`] or [`Self::with_extra`] to attach them.
    ///
    /// Boxed to keep `AwsError` small enough to fit comfortably in a
    /// `Result<_, AwsError>` (clippy's `result_large_err` threshold).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<Box<Map<String, Value>>>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ErrorType {
    Sender,
    Receiver,
}

impl AwsError {
    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// Service-level "resource not found" error returned with HTTP 400.
    ///
    /// Many JSON-protocol services (DynamoDB, KMS, SecretsManager, Cognito, ...)
    /// model `ResourceNotFoundException` and friends as client-side validation
    /// errors and respond with `400 Bad Request` rather than `404 Not Found`.
    /// Use this constructor for those cases; reserve [`Self::not_found`] for
    /// REST-style 404s such as S3's `NoSuchBucket` / `NoSuchKey`.
    pub fn service_not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// HTTP 416 Range Not Satisfiable — used by S3 when a `Range` header
    /// requests bytes outside the object's size.
    pub fn range_not_satisfiable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::RANGE_NOT_SATISFIABLE,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// HTTP 412 Precondition Failed — used by S3 when an `If-Match` /
    /// `If-Unmodified-Since` conditional request fails.
    pub fn precondition_failed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PRECONDITION_FAILED,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// HTTP 429 Too Many Requests — used by services that throttle on
    /// concurrency or rate. Lambda raises this with code
    /// `TooManyRequestsException`; DynamoDB uses
    /// `ProvisionedThroughputExceededException`.
    pub fn too_many_requests(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "InternalServiceError".to_string(),
            message: message.into(),
            error_type: ErrorType::Receiver,
            extras: None,
        }
    }

    pub fn not_implemented(operation: &str) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            code: "NotImplemented".to_string(),
            message: format!("Operation '{operation}' is not yet implemented in AWSim"),
            error_type: ErrorType::Receiver,
            extras: None,
        }
    }

    pub fn unknown_operation(operation: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "UnknownOperationException".to_string(),
            message: format!("Unknown operation: {operation}"),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn access_denied(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "AccessDeniedException".to_string(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// HTTP 403 with a service-specific error code (e.g. Cognito's
    /// `NotAuthorizedException` or SNS's `AuthorizationError`).
    pub fn forbidden(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn access_denied_for(action: &str, principal_arn: &str, resource: &str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "AccessDenied".to_string(),
            message: format!(
                "User: {principal_arn} is not authorized to perform: {action} on resource: {resource}"
            ),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "ValidationException".to_string(),
            message: message.into(),
            error_type: ErrorType::Sender,
            extras: None,
        }
    }

    /// Replace the extras map wholesale.
    pub fn with_extras(mut self, extras: Map<String, Value>) -> Self {
        self.extras = Some(Box::new(extras));
        self
    }

    /// Insert a single extra field, allocating the map if needed.
    pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extras
            .get_or_insert_with(|| Box::new(Map::new()))
            .insert(key.into(), value);
        self
    }
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AwsError {}
