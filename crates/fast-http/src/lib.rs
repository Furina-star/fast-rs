//! Core HTTP primitives, structured ApiError types, and /docs route metadata.

pub mod docs;
pub mod error;
pub mod types;

pub use docs::RouteMeta;
pub use error::ApiError;
pub use types::{Method, Request, Response, StatusCode};
