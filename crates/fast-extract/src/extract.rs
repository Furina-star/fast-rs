//! Typed request extractors (Path, Json, State) and HTTP response conversions.

use std::str::FromStr;
use std::sync::Arc;

use fast_http::{ApiError, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

// Trait for types extracted from an incoming Request.
pub trait FromRequest: Sized {
    // Extracts Self from a borrowed Request or returns an ApiError.
    fn from_request(req: &Request) -> Result<Self, ApiError>;
}

// Trait for handler return values convertible into an HTTP Response.
pub trait IntoResponse {
    // Converts Self into an HTTP Response.
    fn into_response(self) -> Response;
}

// Typed path parameter extractor (e.g. /tasks/{id} -> Path<u32>).
#[derive(Debug, Clone)]
pub struct Path<T>(pub T);

// Typed JSON body extractor and response wrapper.
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

// Shared application state extractor backed by Arc.
#[derive(Debug)]
pub struct State<T>(pub Arc<T>);

impl<T> Clone for State<T> {
    // Clones the underlying Arc handle without cloning the inner state.
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

// --- Core FromRequest implementations ---

impl FromRequest for Request {
    // Clones the incoming Request when a handler asks for raw access.
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        Ok(req.clone())
    }
}

impl<T: DeserializeOwned> FromRequest for Json<T> {
    // Deserializes the raw request body bytes as JSON into T.
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
    // Extracts and parses the first captured path parameter via FromStr.
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        // Grab the first path parameter value from the matched route map.
        let raw = req
            .path_params
            .values()
            .next()
            .ok_or_else(|| ApiError::BadRequest("Missing path parameter".into()))?;

        // Parse into target type T and map parse errors to 400 Bad Request.
        let parsed = raw
            .parse::<T>()
            .map_err(|e| ApiError::BadRequest(format!("Invalid path parameter '{raw}': {e}")))?;

        Ok(Path(parsed))
    }
}

impl<T: Send + Sync + 'static> FromRequest for State<T> {
    // Retrieves registered TypeId-keyed shared state from request extensions.
    fn from_request(req: &Request) -> Result<Self, ApiError> {
        // Look up Arc<T> in request extensions; missing state signals misconfiguration.
        req.get_state::<T>()
            .map(State)
            .ok_or_else(|| ApiError::Internal("Application state not registered".into()))
    }
}

// --- Core IntoResponse implementations ---

impl IntoResponse for Response {
    // Passes an already-constructed Response through unchanged.
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for ApiError {
    // Converts an ApiError into its JSON error Response.
    fn into_response(self) -> Response {
        self.to_response()
    }
}

impl IntoResponse for String {
    // Converts an owned String into a 200 OK plain-text Response.
    fn into_response(self) -> Response {
        Response::text(StatusCode::Ok, self)
    }
}

impl IntoResponse for &'static str {
    // Converts a static string slice into a 200 OK plain-text Response.
    fn into_response(self) -> Response {
        Response::text(StatusCode::Ok, self)
    }
}

impl IntoResponse for (StatusCode, String) {
    // Converts a (StatusCode, String) tuple into a plain-text Response.
    fn into_response(self) -> Response {
        Response::text(self.0, self.1)
    }
}

impl IntoResponse for (StatusCode, &'static str) {
    // Converts a (StatusCode, &'static str) tuple into a plain-text Response.
    fn into_response(self) -> Response {
        Response::text(self.0, self.1)
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    // Serializes Json<T> into a 200 OK JSON Response.
    fn into_response(self) -> Response {
        // Fall back to HTTP 500 ApiError if serialization fails.
        Response::json(StatusCode::Ok, &self.0)
            .unwrap_or_else(|e| ApiError::Internal(e.to_string()).to_response())
    }
}

impl<T: Serialize> IntoResponse for (StatusCode, Json<T>) {
    // Serializes (StatusCode, Json<T>) into a JSON Response with the given status.
    fn into_response(self) -> Response {
        // Fall back to HTTP 500 ApiError if serialization fails.
        Response::json(self.0, &self.1 .0)
            .unwrap_or_else(|e| ApiError::Internal(e.to_string()).to_response())
    }
}

impl<T, E> IntoResponse for Result<T, E>
where
    T: IntoResponse,
    E: Into<ApiError>,
{
    // Routes Ok results to IntoResponse and Err values through Into<ApiError>.
    fn into_response(self) -> Response {
        match self {
            Ok(ok) => ok.into_response(),
            Err(err) => err.into().to_response(),
        }
    }
}
