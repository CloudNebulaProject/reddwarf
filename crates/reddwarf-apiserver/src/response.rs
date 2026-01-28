use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;

/// API response wrapper
pub struct ApiResponse<T: Serialize> {
    status: StatusCode,
    body: T,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a new response with 200 OK
    pub fn ok(body: T) -> Self {
        Self {
            status: StatusCode::OK,
            body,
        }
    }

    /// Create a new response with 201 Created
    pub fn created(body: T) -> Self {
        Self {
            status: StatusCode::CREATED,
            body,
        }
    }

    /// Create a new response with custom status
    pub fn with_status(status: StatusCode, body: T) -> Self {
        Self { status, body }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Create a success Status response
pub fn status_success(message: &str) -> Response {
    Json(json!({
        "apiVersion": "v1",
        "kind": "Status",
        "status": "Success",
        "message": message,
        "code": 200
    }))
    .into_response()
}

/// Create a deletion Status response
pub fn status_deleted(name: &str, kind: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "apiVersion": "v1",
            "kind": "Status",
            "status": "Success",
            "message": format!("{} {} deleted", kind, name),
            "code": 200
        })),
    )
        .into_response()
}
