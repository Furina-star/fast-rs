//! Core HTTP primitives, structured ApiError types, and /docs route metadata.

pub mod docs;
pub mod error;
pub mod types;

pub use docs::{build_docs_response, build_swagger_html, RouteMeta};
pub use error::ApiError;
pub use types::{parse_query_string, Method, Request, Response, StatusCode};

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies query string parsing and URL space decoding.
    #[test]
    fn parses_query_strings() {
        let query = parse_query_string("?done=true&title=hello%20world&tag=fast+rs");
        assert_eq!(query.get("done").map(String::as_str), Some("true"));
        assert_eq!(query.get("title").map(String::as_str), Some("hello world"));
        assert_eq!(query.get("tag").map(String::as_str), Some("fast rs"));
    }

    // Verifies header lookup ignores letter casing.
    #[test]
    fn matches_headers_case_insensitively() {
        let mut req = Request::new(Method::Get, "/tasks");
        req.headers
            .insert("Content-Type".into(), "application/json".into());
        assert_eq!(req.header("content-type"), Some("application/json"));
        assert_eq!(req.header("CONTENT-TYPE"), Some("application/json"));
    }

    // Verifies HTTP/1.1 wire formatting and CORS headers.
    #[test]
    fn formats_http_response_bytes() {
        let res = Response::text(StatusCode::Ok, "hi").with_cors();
        let raw = String::from_utf8(res.to_http_bytes()).unwrap();
        assert!(raw.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(raw.contains("Access-Control-Allow-Origin: *"));
        assert!(raw.ends_with("\r\n\r\nhi"));
    }

    // Verifies ApiError maps to the right HTTP status code and JSON body.
    #[test]
    fn converts_api_error_to_json_response() {
        let res = ApiError::NotFound("Task 42 not found".into()).to_response();
        let body = String::from_utf8(res.body).unwrap();
        assert_eq!(res.status, StatusCode::NotFound);
        assert!(body.contains("Task 42 not found"));
    }

    // Verifies OpenAPI 3.0 JSON detects path parameters and Swagger UI loads /openapi.json.
    #[test]
    fn builds_openapi_spec_and_swagger_ui() {
        let routes = vec![RouteMeta {
            method: Method::Get,
            path: "/tasks/{id}".into(),
            handler_name: "get_task".into(),
        }];

        let spec_res = build_docs_response(&routes);
        let spec_json: serde_json::Value = serde_json::from_slice(&spec_res.body).unwrap();
        assert_eq!(spec_json["openapi"], "3.0.0");
        assert_eq!(
            spec_json["paths"]["/tasks/{id}"]["get"]["parameters"][0]["name"],
            "id"
        );

        let html_res = build_swagger_html();
        let html = String::from_utf8(html_res.body).unwrap();
        assert!(html.contains("/openapi.json"));
    }
}
