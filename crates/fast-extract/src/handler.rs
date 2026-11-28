//! Variadic async Handler trait and type erasure into dynamic BoxedHandler closures.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fast_http::{Method, Request, Response, RouteMeta};

use crate::extract::{FromRequest, IntoResponse};

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

// Holds route metadata and its type-erased async handler for router registration.
#[derive(Clone)]
pub struct Route {
    pub method: Method,
    pub path: &'static str,
    pub handler_name: &'static str,
    pub handler: BoxedHandler,
}

impl Route {
    // Bundles an HTTP method, path, name, and typed Handler into a uniform Route.
    pub fn new<H, Args>(
        method: Method,
        path: &'static str,
        handler_name: &'static str,
        handler: H,
    ) -> Self
    where
        H: Handler<Args>,
    {
        Self {
            method,
            path,
            handler_name,
            handler: into_boxed_handler(handler),
        }
    }

    // Convers this route's metadata into a RouteMeta entry for /openai.json
    pub fn meta(&self) -> RouteMeta {
        RouteMeta {
            method: self.method,
            path: self.path.to_string(),
            handler_name: self.handler_name.to_string(),
        }
    }
}

// 0-argument handler iml: 1...=3 argument impls are generated via macro_rules!.
impl<F, Fut, Res> Handler<()> for F
where F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoResponse,
{
    // Boxes the zero-arugment handler future and maps ints output via IntoResponse.
    fn call(&self, _req: Request) -> BoxFuture<'static, Response> {
        let fut = (self)();
        Box::pin(async move { fut.await.into_response() })
    }
}

// Generates Handler<(T1, ...)> implementations for async functions with 1 or more extractors.
macro_rules! impl_handler {
    ($($T:ident), +) => {
        #[allow(non_snake_case)]
        impl<F, Fut, Res, $($T),+> Handler<($($T,)+)> for F
        where
            F: Fn($($T), +) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send + 'static,
            Res: IntoResponse,
            $($T: FromRequest,)+
        {
            // Extracts every argument from the request, short-circuiting on ApiError.
            fn call(&self, req: Request) -> BoxFuture<'static, Response> {
                $(
                    let $T = match $T::from_request(&req) {
                        Ok(val) => val,
                        Err(err) => return Box::pin(async move { err.to_response() }),
                    };
                )+

                let fut = (self)($($T),+);
                Box::pin(async move { fut.await.into_response() })
            }
        }
    };
}

impl_handler!(T1);
impl_handler!(T1, T2);
impl_handler!(T1, T2, T3);