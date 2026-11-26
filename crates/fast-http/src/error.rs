use std::fmt;

use serde_json::json;

use crate::types::{Response, StatusCode};

/// Mirrors FastAPI's HTTPException as an algebraic data type usable with `?`.
#[derive(Debug, Clone)]
pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    UnprocessableEntity(String),
    Internal(String),
}

impl ApiError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BadRequest,
            ApiError::NotFound(_) => StatusCode::NotFound,
            ApiError::UnprocessableEntity(_) => StatusCode::UnprocessableEntity,
            ApiError::Internal(_) => StatusCode::InternalServerError,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            ApiError::BadRequest(msg)
            | ApiError::NotFound(msg)
            | ApiError::UnprocessableEntity(msg)
            | ApiError::Internal(msg) => msg,
        }
    }

    pub fn to_response(&self) -> Response {
        let payload = json!({ "detail": self.message() });
        Response::json(self.status_code(), &payload).unwrap_or_else(|_| {
            Response::text(StatusCode::InternalServerError, "Serialization error")
        })
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status_code().as_u16(), self.message())
    }
}

impl std::error::Error for ApiError {}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::UnprocessableEntity(format!("Invalid JSON payload: {err}"))
    }
}

impl From<std::num::ParseIntError> for ApiError {
    fn from(err: std::num::ParseIntError) -> Self {
        ApiError::BadRequest(format!("Invalid integer parameter: {err}"))
    }
}
