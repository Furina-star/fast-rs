//! Structured HTTP error type mirroring FastAPI's HTTPException with ? propagation.

use std::fmt;

use serde_json::json;

use crate::types::{Response, StatusCode};

// Structured HTTP error enum supporting ? propagation in route handlers.
#[derive(Debug, Clone)]
pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    UnprocessableEntity(String),
    Internal(String),
}

impl ApiError {
    // Maps each error variant to its corresponding HTTP status code.
    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BadRequest,
            ApiError::NotFound(_) => StatusCode::NotFound,
            ApiError::UnprocessableEntity(_) => StatusCode::UnprocessableEntity,
            ApiError::Internal(_) => StatusCode::InternalServerError,
        }
    }

    // Returns the human-readable detail message carried by this error.
    pub fn message(&self) -> &str {
        match self {
            ApiError::BadRequest(msg)
            | ApiError::NotFound(msg)
            | ApiError::UnprocessableEntity(msg)
            | ApiError::Internal(msg) => msg,
        }
    }

    // Formats the error as a FastAPI-style {"detail": ...} JSON Response.
    pub fn to_response(&self) -> Response {
        let payload = json!({ "detail": self.message() });
        // Fall back to HTTP 500 plain text if JSON serialization fails.
        Response::json(self.status_code(), &payload).unwrap_or_else(|_| {
            Response::text(StatusCode::InternalServerError, "Serialization error")
        })
    }
}

impl fmt::Display for ApiError {
    // Formats the error as "<status>: <detail>" for logging and Error traits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status_code().as_u16(), self.message())
    }
}

impl std::error::Error for ApiError {}

impl From<serde_json::Error> for ApiError {
    // Maps malformed JSON deserialization errors to 422 Unprocessable Entity.
    fn from(err: serde_json::Error) -> Self {
        ApiError::UnprocessableEntity(format!("Invalid JSON payload: {err}"))
    }
}

impl From<std::num::ParseIntError> for ApiError {
    // Maps integer parsing failures to 400 Bad Request.
    fn from(err: std::num::ParseIntError) -> Self {
        ApiError::BadRequest(format!("Invalid integer parameter: {err}"))
    }
}
