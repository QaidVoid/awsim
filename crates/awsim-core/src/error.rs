use axum::http::StatusCode;
use serde::Serialize;

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
        }
    }

    pub fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: code.into(),
            message: message.into(),
            error_type: ErrorType::Sender,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "InternalServiceError".to_string(),
            message: message.into(),
            error_type: ErrorType::Receiver,
        }
    }

    pub fn not_implemented(operation: &str) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            code: "NotImplemented".to_string(),
            message: format!("Operation '{operation}' is not yet implemented in AWSim"),
            error_type: ErrorType::Receiver,
        }
    }

    pub fn unknown_operation(operation: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "UnknownOperationException".to_string(),
            message: format!("Unknown operation: {operation}"),
            error_type: ErrorType::Sender,
        }
    }

    pub fn access_denied(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "AccessDeniedException".to_string(),
            message: message.into(),
            error_type: ErrorType::Sender,
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
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "ValidationException".to_string(),
            message: message.into(),
            error_type: ErrorType::Sender,
        }
    }
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AwsError {}
