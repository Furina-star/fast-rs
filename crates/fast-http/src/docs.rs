use serde::Serialize;

use crate::types::{Method, Response, StatusCode};

/// Metadata registered for each route, used to serve `GET /docs`.
#[derive(Debug, Clone, Serialize)]
pub struct RouteMeta {
    pub method: Method,
    pub path: String,
    pub handler_name: String,
}

/// Starter manifest generator (Good Dev expands this on `feat/http-docs`).
pub fn build_docs_response(routes: &[RouteMeta]) -> Response {
    let payload = serde_json::json!({
        "title": "fast-rs API",
        "version": "0.1.0",
        "routes": routes,
    });
    Response::json(StatusCode::Ok, &payload)
        .unwrap_or_else(|_| Response::text(StatusCode::InternalServerError, "Failed to build docs"))
}
