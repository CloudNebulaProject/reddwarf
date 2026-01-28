use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// API error type
#[derive(Debug)]
pub enum ApiError {
    /// Resource not found (404)
    NotFound(String),

    /// Resource already exists (409)
    AlreadyExists(String),

    /// Conflict - concurrent modification (409)
    Conflict(String),

    /// Invalid input (400)
    BadRequest(String),

    /// Internal server error (500)
    Internal(String),

    /// Validation failed (422)
    ValidationFailed(String),

    /// Unsupported media type (415)
    UnsupportedMediaType(String),

    /// Method not allowed (405)
    MethodNotAllowed(String),
}

/// Result type for API operations
pub type Result<T> = std::result::Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::AlreadyExists(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::ValidationFailed(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            ApiError::UnsupportedMediaType(msg) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, msg),
            ApiError::MethodNotAllowed(msg) => (StatusCode::METHOD_NOT_ALLOWED, msg),
        };

        let body = Json(json!({
            "apiVersion": "v1",
            "kind": "Status",
            "status": "Failure",
            "message": message,
            "code": status.as_u16()
        }));

        (status, body).into_response()
    }
}

impl From<reddwarf_core::ReddwarfError> for ApiError {
    fn from(err: reddwarf_core::ReddwarfError) -> Self {
        use reddwarf_core::ReddwarfError;

        match err {
            ReddwarfError::ResourceNotFound { .. } => ApiError::NotFound(err.to_string()),
            ReddwarfError::ResourceAlreadyExists { .. } => ApiError::AlreadyExists(err.to_string()),
            ReddwarfError::Conflict { .. } => ApiError::Conflict(err.to_string()),
            ReddwarfError::ValidationFailed { .. } => ApiError::ValidationFailed(err.to_string()),
            ReddwarfError::InvalidResource { .. } => ApiError::BadRequest(err.to_string()),
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<reddwarf_storage::StorageError> for ApiError {
    fn from(err: reddwarf_storage::StorageError) -> Self {
        use reddwarf_storage::StorageError;

        match err {
            StorageError::KeyNotFound { .. } => ApiError::NotFound(err.to_string()),
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<reddwarf_versioning::VersioningError> for ApiError {
    fn from(err: reddwarf_versioning::VersioningError) -> Self {
        use reddwarf_versioning::VersioningError;

        match err {
            VersioningError::Conflict { .. } => ApiError::Conflict(err.to_string()),
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("JSON error: {}", err))
    }
}
