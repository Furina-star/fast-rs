use std::str::FromStr;
use std::sync::Arc;

use fast_http::{ApiError, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Implemented by any type that can be extracted from an incoming `Request`.
pub trait FromRequest: Sized {
    fn from_request(req: &Request) -> Result<Self, ApiError>;
}

/// Implemented by any type a route handler can return as an HTTP `Response`.
pub trait IntoResponse {
    fn into_response(self) -> Response;
}

/// Extracts a typed path parameter (e.g., `/tasks/{id}` -> `Path<u32>`).
#[derive(Debug, Clone)]
pub struct Path<T>(pub T);

/// Extracts or returns a typed JSON body (mirrors FastAPI's Pydantic models).
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

/// Extracts shared application state (mirrors FastAPI's `Depends` injection).
#[derive(Debug)]
pub struct State<T>(pub Arc<T>);

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

// --- Core FromRequest implementations ---

impl FromRequest for Request {
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        Ok(req.clone())
    }
}

impl<T: DeserializeOwned> FromRequest for Json<T> {
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        let value = serde_json::from_slice::<T>(&req.body)?;
        Ok(Json(value))
    }
}

impl<T> FromRequest for Path<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        let raw = req
            .path_params
            .values()
            .next()
            .ok_or_else(|| ApiError::BadRequest("Missing path parameter".into()))?;

        let parsed = raw
            .parse::<T>()
            .map_err(|e| ApiError::BadRequest(format!("Invalid path parameter '{raw}': {e}")))?;

        Ok(Path(parsed))
    }
}

impl<T: Send + Sync + 'static> FromRequest for State<T> {
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        req.get_state::<T>()
            .map(State)
            .ok_or_else(|| ApiError::Internal("Application state not registered".into()))
    }
}

// --- Core IntoResponse implementations ---

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.to_response()
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        Response::text(StatusCode::Ok, self)
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        Response::text(StatusCode::Ok, self)
    }
}

impl IntoResponse for (StatusCode, String) {
    fn into_response(self) -> Response {
        Response::text(self.0, self.1)
    }
}

impl IntoResponse for (StatusCode, &'static str) {
    fn into_response(self) -> Response {
        Response::text(self.0, self.1)
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        Response::json(StatusCode::Ok, &self.0)
            .unwrap_or_else(|e| ApiError::Internal(e.to_string()).to_response())
    }
}

impl<T: Serialize> IntoResponse for (StatusCode, Json<T>) {
    fn into_response(self) -> Response {
        Response::json(self.0, &self.1 .0)
            .unwrap_or_else(|e| ApiError::Internal(e.to_string()).to_response())
    }
}

impl<T, E> IntoResponse for Result<T, E>
where
    T: IntoResponse,
    E: Into<ApiError>,
{
    fn into_response(self) -> Response {
        match self {
            Ok(ok) => ok.into_response(),
            Err(err) => err.into().to_response(),
        }
    }
}
