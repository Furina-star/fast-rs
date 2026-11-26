use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fast_http::{Request, Response};

use crate::extract::IntoResponse;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Type-erased handler stored inside `fast-core`'s router table.
pub type BoxedHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;

/// Implemented for async functions taking 0..=3 `FromRequest` parameters.
pub trait Handler<Args>: Send + Sync + 'static {
    fn call(&self, req: Request) -> BoxFuture<'static, Response>;
}

/// Converts any typed `Handler<Args>` into a `BoxedHandler` for dynamic routing.
pub fn into_boxed_handler<H, Args>(handler: H) -> BoxedHandler
where
    H: Handler<Args>,
{
    Arc::new(move |req: Request| handler.call(req))
}

// 0-argument handler contract (Skilled #2 implements 1..=3 args via `macro_rules!` on `feat/macros-extract`)
impl<F, Fut, Res> Handler<()> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoResponse,
{
    fn call(&self, _req: Request) -> BoxFuture<'static, Response> {
        let fut = (self)();
        Box::pin(async move { fut.await.into_response() })
    }
}
