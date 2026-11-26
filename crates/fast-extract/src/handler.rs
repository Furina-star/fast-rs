//! Variadic async Handler trait and type erasure into dynamic BoxedHandler closures.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fast_http::{Request, Response};

use crate::extract::IntoResponse;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// Type-erased handler stored in fast-core's route table.
pub type BoxedHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;

// Trait for async handler functions taking 0..=3 FromRequest arguments.
pub trait Handler<Args>: Send + Sync + 'static {
    // Invokes the handler with an incoming Request and returns a boxed response future.
    fn call(&self, req: Request) -> BoxFuture<'static, Response>;
}

// Wraps a typed Handler<Args> in an Arc closure for dynamic router dispatch.
pub fn into_boxed_handler<H, Args>(handler: H) -> BoxedHandler
where
    H: Handler<Args>,
{
    Arc::new(move |req: Request| handler.call(req))
}

// 0-argument handler impl; 1..=3 argument impls are generated via macro_rules!.
impl<F, Fut, Res> Handler<()> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoResponse,
{
    // Boxes the zero-argument handler future and maps its output via IntoResponse.
    fn call(&self, _req: Request) -> BoxFuture<'static, Response> {
        let fut = (self)();
        Box::pin(async move { fut.await.into_response() })
    }
}
