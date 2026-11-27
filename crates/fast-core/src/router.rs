//! Slice-based route matcher extracting path parameters and injecting shared state.
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use fast_extract::BoxedHandler;
use fast_http::docs::{build_docs_response, build_swagger_html};
use fast_http::{ApiError, Method, Request, Response, RouteMeta};

struct Route {
    meta: RouteMeta,
    handler: BoxedHandler,
}

/// Registered handlers and typed state shared by the application.
#[derive(Default)]
pub struct App {
    routes: Vec<Route>,
    state: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route(&mut self, meta: RouteMeta, handler: BoxedHandler) -> &mut Self {
        self.routes.push(Route { meta, handler });
        self
    }

    pub fn insert_state<T: Send + Sync + 'static>(&mut self, value: Arc<T>) -> &mut Self {
        self.state.insert(TypeId::of::<T>(), value);
        self
    }

    pub async fn dispatch(&self, mut req: Request) -> Response {
        if req.method == Method::Get {
            match req.path.as_str() {
                "/docs" => return build_swagger_html(),
                "/openapi.json" => {
                    let metadata = self
                        .routes
                        .iter()
                        .map(|route| &route.meta)
                        .filter(|meta| {
                            meta.method != Method::Get
                                || !matches!(meta.path.as_str(), "/docs" | "/openapi.json")
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    return build_docs_response(&metadata);
                }
                _ => {}
            }
        }

        let routes = self
            .routes
            .iter()
            .filter(|route| route.meta.method == req.method);
        let literal = routes.clone().find(|route| {
            route.meta.path == req.path
                && !route
                    .meta
                    .path
                    .split('/')
                    .any(|segment| parameter_name(segment).is_some())
        });

        let route = match literal {
            Some(route) => route,
            None => {
                let Some(route) = routes
                    .into_iter()
                    .find(|route| matches_path(&route.meta.path, &req.path))
                else {
                    return ApiError::NotFound("Route not found".into()).to_response();
                };
                req.path_params.extend(
                    route
                        .meta
                        .path
                        .split('/')
                        .zip(req.path.split('/'))
                        .filter_map(|(pattern, segment)| {
                            parameter_name(pattern)
                                .map(|name| (name.to_owned(), segment.to_owned()))
                        }),
                );
                route
            }
        };

        req.extensions.extend(
            self.state
                .iter()
                .map(|(&key, value)| (key, Arc::clone(value))),
        );
        (route.handler)(req).await
    }
}

fn parameter_name(segment: &str) -> Option<&str> {
    let name = segment.strip_prefix('{')?.strip_suffix('}')?;
    (!name.is_empty() && !name.contains(['{', '}'])).then_some(name)
}

fn matches_path(template: &str, path: &str) -> bool {
    let mut path_segments = path.split('/');
    for pattern in template.split('/') {
        let Some(segment) = path_segments.next() else {
            return false;
        };
        if parameter_name(pattern).is_some() {
            if segment.is_empty() {
                return false;
            }
        } else if pattern != segment {
            return false;
        }
    }
    path_segments.next().is_none()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use fast_extract::{into_boxed_handler, BoxedHandler};
    use fast_http::{Method, Request, Response, RouteMeta, StatusCode};

    use super::App;

    fn get(app: &mut App, path: &str, handler: BoxedHandler) {
        app.route(
            RouteMeta {
                method: Method::Get,
                path: path.into(),
                handler_name: "test_handler".into(),
            },
            handler,
        );
    }

    #[tokio::test]
    async fn built_in_docs_use_registered_metadata() {
        let mut app = App::new();
        for path in ["/tasks/{id}", "/docs", "/openapi.json"] {
            get(
                &mut app,
                path,
                into_boxed_handler(|| async { "user route" }),
            );
        }

        let docs = app.dispatch(Request::new(Method::Get, "/docs")).await;
        assert_eq!(docs.status, StatusCode::Ok);
        assert!(docs.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type") && value.starts_with("text/html")
        }));
        assert!(String::from_utf8(docs.body)
            .unwrap()
            .contains("/openapi.json"));

        let schema = app
            .dispatch(Request::new(Method::Get, "/openapi.json"))
            .await;
        assert_eq!(schema.status, StatusCode::Ok);
        assert!(schema.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type") && value.starts_with("application/json")
        }));
        let body = String::from_utf8(schema.body).unwrap();
        let openapi = body.split_once("\"openapi\"").unwrap().1;
        assert!(openapi
            .trim_start()
            .strip_prefix(':')
            .unwrap()
            .trim_start()
            .starts_with("\"3.0."));
        assert!(body.contains("\"/tasks/{id}\""));
        assert!(!body.contains("\"/docs\""));
        assert!(!body.contains("\"/openapi.json\""));
    }

    #[tokio::test]
    async fn dynamic_route_injects_params_and_state() {
        let state = Arc::new(RwLock::new(vec![42_u32]));
        let expected = Arc::clone(&state);
        let mut app = App::new();
        app.insert_state(state);
        get(
            &mut app,
            "/tasks/{id}",
            Arc::new(move |req| {
                assert_eq!(req.path_params.get("id").map(String::as_str), Some("42"));
                assert!(Arc::ptr_eq(&req.get_state().unwrap(), &expected));
                assert_eq!(req.get_state::<u8>().as_deref(), Some(&7));
                Box::pin(async { Response::text(StatusCode::Ok, "matched") })
            }),
        );

        let mut req = Request::new(Method::Get, "/tasks/42");
        req.insert_state(Arc::new(7_u8));
        assert_eq!(app.dispatch(req).await.status, StatusCode::Ok);
    }

    #[tokio::test]
    async fn literal_route_wins() {
        let mut app = App::new();
        get(
            &mut app,
            "/tasks/{id}",
            into_boxed_handler(|| async { "template" }),
        );
        get(
            &mut app,
            "/tasks/latest",
            into_boxed_handler(|| async { "literal" }),
        );

        let response = app
            .dispatch(Request::new(Method::Get, "/tasks/latest"))
            .await;
        assert_eq!(response.body, b"literal");
    }

    #[tokio::test]
    async fn failed_match_leaves_no_captures() {
        let mut app = App::new();
        get(
            &mut app,
            "/tasks/{id}/edit",
            into_boxed_handler(|| async { "edit" }),
        );
        get(
            &mut app,
            "/tasks/{slug}/view",
            Arc::new(|req| {
                assert_eq!(req.path_params.len(), 1);
                assert_eq!(req.path_params.get("slug").map(String::as_str), Some("42"));
                Box::pin(async { Response::text(StatusCode::Ok, "view") })
            }),
        );

        let response = app
            .dispatch(Request::new(Method::Get, "/tasks/42/view"))
            .await;
        assert_eq!(response.body, b"view");
    }

    #[tokio::test]
    async fn unmatched_routes_return_404() {
        let mut app = App::new();
        get(
            &mut app,
            "/tasks/{id}",
            into_boxed_handler(|| async { "template" }),
        );

        for (method, path) in [
            (Method::Get, "/missing"),
            (Method::Get, "/tasks/"),
            (Method::Get, "/tasks/42/"),
            (Method::Post, "/tasks/42"),
        ] {
            let response = app.dispatch(Request::new(method, path)).await;
            assert_eq!(response.status, StatusCode::NotFound, "{method} {path}");
        }
    }
}
