//! Request extractors, response conversions, and type-erased async handler traits.

pub mod extract;
pub mod handler;

pub use extract::{FromRequest, IntoResponse, Json, Path, State};
pub use handler::{into_boxed_handler, BoxFuture, BoxedHandler, Handler, Route};
