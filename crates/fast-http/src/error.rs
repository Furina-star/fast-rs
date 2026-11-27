//! Structured HTTP error type mirroring FastAPI's HTTPException with ? propagation.

use std::fmt;

use serde_json::json;

use crate::types::{Response, StatusCode};

// Represents HTTP errors as an enum so route handlers can return them with `?`.
#[derive(Debug, Clone)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    MethodNotAllowed(String),
    UnprocessableEntity(String),
    Internal(String),
}

impl ApiError {
    // Maps each error variant to its matching HTTP status code.
    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BadRequest,
            ApiError::Unauthorized(_) => StatusCode::Unauthorized,
            ApiError::Forbidden(_) => StatusCode::Forbidden,
            ApiError::NotFound(_) => StatusCode::NotFound,
            ApiError::MethodNotAllowed(_) => StatusCode::MethodNotAllowed,
            ApiError::UnprocessableEntity(_) => StatusCode::UnprocessableEntity,
            ApiError::Internal(_) => StatusCode::InternalServerError,
        }
    }

    // Extracts the inner error message slice from any variant.
    pub fn message(&self) -> &str {
        match self {
            ApiError::BadRequest(msg)
            | ApiError::Unauthorized(msg)
            | ApiError::Forbidden(msg)
            | ApiError::NotFound(msg)
            | ApiError::MethodNotAllowed(msg)
            | ApiError::UnprocessableEntity(msg)
            | ApiError::Internal(msg) => msg,
        }
    }

    // Converts this error into FastAPI-style JSON response with CORS headers.
    pub fn to_response(&self) -> Response {
        let payload = json!({ "detail": self.message() });
        Response::json(self.status_code(), &payload)
            .unwrap_or_else(|_| {
                Response::text(StatusCode::InternalServerError, "Serialization error")
            })
            .with_cors()
    }
}

impl fmt::Display for ApiError {
    // Formats the error as "<status>: <message>" for logs and error strings.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status_code().as_u16(), self.message())
    }
}

impl std::error::Error for ApiError {}

// Auto-converts serde_json parsing failures into HTTP 422 errors when using `?`.
impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::UnprocessableEntity(format!("Invalid JSON payload: {err}"))
    }
}

// Auto-converts integer parsing failures into HTTP 400 errors when using `?`.
impl From<std::num::ParseIntError> for ApiError {
    fn from(err: std::num::ParseIntError) -> Self {
        ApiError::BadRequest(format!("Invalid integer parameter: {err}"))
    }
}