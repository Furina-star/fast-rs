pub use fast_core as core;
pub use fast_extract as extract;
pub use fast_http as http;
pub use fast_macros as macros;

pub mod prelude {
    pub use fast_extract::{
        into_boxed_handler, BoxFuture, BoxedHandler, FromRequest, Handler, IntoResponse, Json,
        Path, State,
    };
    pub use fast_http::{ApiError, Method, Request, Response, RouteMeta, StatusCode};
}