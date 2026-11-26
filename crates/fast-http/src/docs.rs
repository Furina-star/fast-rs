//! Route metadata and automatic GET /docs JSON manifest generation.

use serde::Serialize;

use crate::types::{Method, Response, StatusCode};

// Metadata registered for each route and exposed by GET /docs.
#[derive(Debug, Clone, Serialize)]
pub struct RouteMeta {
    pub method: Method,
    pub path: String,
    pub handler_name: String,
}

// Builds the JSON route manifest response for GET /docs.
pub fn build_docs_response(routes: &[RouteMeta]) -> Response {
    let payload = serde_json::json!({
        "title": "fast-rs API",
        "version": "0.1.0",
        "routes": routes,
    });
    // Fall back to HTTP 500 plain text if manifest serialization fails.
    Response::json(StatusCode::Ok, &payload)
        .unwrap_or_else(|_| Response::text(StatusCode::InternalServerError, "Failed to build docs"))
}
