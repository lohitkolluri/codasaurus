use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// API error types for all endpoint handlers.
#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
    Unauthorized(String),
    Forbidden(String),
    /// message, `Retry-After` seconds (0 = omit the header)
    TooManyRequests(String, u64),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message, retry_after) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg, 0),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg, 0),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg, 0),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg, 0),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg, 0),
            ApiError::TooManyRequests(msg, secs) => (StatusCode::TOO_MANY_REQUESTS, msg, *secs),
        };
        let body = (status, Json(json!({ "error": message }))).into_response();
        if retry_after > 0 {
            let mut body = body;
            body.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_str(&retry_after.to_string())
                    .unwrap_or_else(|_| axum::http::HeaderValue::from_static("60")),
            );
            body
        } else {
            body
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Internal(err.to_string())
    }
}

// Convenience constructors so handlers read naturally.
impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        ApiError::NotFound(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        ApiError::Unauthorized(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        ApiError::Forbidden(msg.into())
    }

    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        ApiError::TooManyRequests(msg.into(), 0)
    }

    pub fn too_many_requests_after(msg: impl Into<String>, retry_after_secs: u64) -> Self {
        ApiError::TooManyRequests(msg.into(), retry_after_secs)
    }
}
