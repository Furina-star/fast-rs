//! Root facade crate re-exporting all fast-rs workspace crates and prelude.

pub use fast_core as core;
pub use fast_extract as extract;
pub use fast_http as http;
pub use fast_macros as macros;

// Top-level re-exports required by generated #[get], #[post], and #[delete] code.
pub use fast_extract::Route;
pub use fast_http::Method;
pub use fast_macros::{delete, get, post};

// Common traits, extractors, macros, and HTTP types for ergonomic imports.
pub mod prelude {
    pub use fast_extract::{
        into_boxed_handler, BoxFuture, BoxedHandler, FromRequest, Handler, IntoResponse, Json,
        Path, Route, State,
    };
    pub use fast_http::{
        build_docs_response, build_swagger_html, parse_query_string, ApiError, Method, Request,
        Response, RouteMeta, StatusCode,
    };
    pub use fast_macros::{delete, get, post};
}
