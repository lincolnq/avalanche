//! Server error type with HTTP status mapping.
//!
//! [`ServerError`] is the single error type returned by all route handlers.
//! Its [`IntoResponse`] impl maps each variant to an HTTP status code.
//!
//! # Security note
//!
//! Error responses intentionally return generic messages ("not found",
//! "unauthorized") rather than leaking internal details. Database errors and
//! internal failures are logged server-side via `tracing::error` but the
//! client only sees "internal error".

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StaleDeviceRef {
    pub did: String,
    pub device_id: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("rate limited")]
    RateLimited,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("stale device")]
    StaleDevice(Vec<StaleDeviceRef>),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        match self {
            ServerError::StaleDevice(stale) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "stale_device",
                    "stale_devices": stale,
                })),
            )
                .into_response(),
            ServerError::Db(e) => {
                tracing::error!("database error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
            ServerError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            ServerError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
            ServerError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg).into_response(),
            ServerError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            ServerError::RateLimited => {
                (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response()
            }
            ServerError::Conflict(msg) => (StatusCode::CONFLICT, msg).into_response(),
            ServerError::Internal(msg) => {
                tracing::error!("internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}
