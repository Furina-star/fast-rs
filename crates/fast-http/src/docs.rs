//! Route metadata and automatic GET /docs JSON manifest generation.

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::types::{Method, Response, StatusCode};

// Metadata for a registered route, used to generate OpenAPI documentation.
#[derive(Debug, Clone, Serialize)]
pub struct RouteMeta {
    pub method: Method,
    pub path: String,
    pub handler_name: String,
}

// Extracts "{param}" placeholders from a route path into OpenAPI parameter objects.
fn extract_path_params(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| {
            json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            })
        })
        .collect()
}

// Generates an OpenAPI 3.0 JSON specification served at GET /openapi.json.
pub fn build_docs_response(routes: &[RouteMeta]) -> Response {
    let mut paths: Map<String, Value> = Map::new();

    for route in routes {
        let method_key = route.method.as_str().to_ascii_lowercase();
        let params = extract_path_params(&route.path);

        let mut operation = json!({
            "operationId": route.handler_name,
            "responses": {
                "200": { "description": "Successful Response" }
            }
        });

        if !params.is_empty() {
            operation["parameters"] = Value::Array(params);
        }

        if matches!(route.method, Method::Post | Method::Put | Method::Patch) {
            operation["requestBody"] = json!({
                "required": true,
                "content": {
                    "application/json": {
                        "schema": { "type": "object" }
                    }
                }
            });
        }

        // Group multiple HTTP methods (e.g., GET and POST) under the same URL path key.
        let path_entry = paths
            .entry(route.path.clone())
            .or_insert_with(|| Value::Object(Map::new()));

        if let Some(methods_map) = path_entry.as_object_mut() {
            methods_map.insert(method_key, operation);
        }
    }

    let spec = json!({
        "openapi": "3.0.0",
        "info": { "title": "fast-rs API", "version": "0.1.0" },
        "paths": paths,
    });

    Response::json(StatusCode::Ok, &spec)
        .unwrap_or_else(|_| {
            Response::text(
                StatusCode::InternalServerError,
                "Failed to build OpenAPI spec",
            )
        })
        .with_cors()
}

// Serves an interactive FastAPI-style Swagger UI webpage at GET /docs.
pub fn build_swagger_html() -> Response {
    const HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>fast-rs — Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({ url: '/openapi.json', dom_id: '#swagger-ui' });
  </script>
</body>
</html>"#;

    Response::html(StatusCode::Ok, HTML)
}
