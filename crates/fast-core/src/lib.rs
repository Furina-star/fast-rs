//! Core async runtime crate: HTTP parser, route matcher, and Tokio TCP server.

pub mod parser;
pub mod router;
pub mod server;

pub use router::App;