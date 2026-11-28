use std::sync::Arc;

use fast_rs::prelude::*;
use serde::{Deserialize, Serialize};

// Shared app config used to test the State<T> extractor.
struct AppConfig {
    prefix: String,
}

#[derive(Deserialize)]
struct CreateTask {
    title: String,
}

#[derive(Serialize)]
struct TaskReply {
    id: u32,
    title: String,
}

// 0-argument GET route.
#[get("/ping")]
async fn ping() -> &'static str {
    "pong"
}

// 1-argument GET route extracting a typed u32 from /tasks/{id}.
#[get("/tasks/{id}")]
async fn get_task(Path(id): Path<u32>) -> Json<TaskReply> {
    Json(TaskReply {
        id,
        title: format!("Task #{id}"),
    })
}

// 2-argument POST route extracting shared State and a JSON body.
#[post("/tasks")]
async fn create_task(
    State(cfg): State<AppConfig>,
    Json(body): Json<CreateTask>,
) -> (StatusCode, Json<TaskReply>) {
    let reply = TaskReply {
        id: 1,
        title: format!("{}: {}", cfg.prefix, body.title),
    };
    (StatusCode::Created, Json(reply))
}

// 1-argument DELETE route returning 204 No Content.
#[delete("/tasks/{id}")]
async fn delete_task(Path(_id): Path<u32>) -> (StatusCode, &'static str) {
    (StatusCode::NoContent, "")
}

// Verifies #[get] creates a valid 0-argument Route and returns 200 OK.
#[tokio::test]
async fn get_macro_handles_zero_arg_route() {
    let route = ping();
    assert_eq!(route.method, Method::Get);
    assert_eq!(route.path, "/ping");
    assert_eq!(route.handler_name, "ping");

    let res = (route.handler)(Request::new(Method::Get, "/ping")).await;
    assert_eq!(res.status, StatusCode::Ok);
    assert_eq!(String::from_utf8(res.body).unwrap(), "pong");
}

// Verifies Path<u32> parses valid IDs and returns 400 Bad Request for invalid text.
#[tokio::test]
async fn get_macro_extracts_path_and_rejects_bad_types() {
    let route = get_task();

    let mut ok_req = Request::new(Method::Get, "/tasks/42");
    ok_req.path_params.insert("id".into(), "42".into());
    let ok_res = (route.handler)(ok_req).await;
    assert_eq!(ok_res.status, StatusCode::Ok);
    assert!(String::from_utf8(ok_res.body).unwrap().contains("Task #42"));

    let mut bad_req = Request::new(Method::Get, "/tasks/abc");
    bad_req.path_params.insert("id".into(), "abc".into());
    let bad_res = (route.handler)(bad_req).await;
    assert_eq!(bad_res.status, StatusCode::BadRequest);
}

// Verifies 2-arg macro_rules! extracts State + Json and returns 422 on broken JSON.
#[tokio::test]
async fn post_macro_extracts_state_and_json() {
    let route = create_task();
    assert_eq!(route.method, Method::Post);

    let config = Arc::new(AppConfig {
        prefix: "TODO".into(),
    });

    let mut ok_req = Request::new(Method::Post, "/tasks");
    ok_req.insert_state(Arc::clone(&config));
    ok_req.body = br#"{"title":"Ship fast-rs"}"#.to_vec();
    let ok_res = (route.handler)(ok_req).await;
    assert_eq!(ok_res.status, StatusCode::Created);
    assert!(String::from_utf8(ok_res.body)
        .unwrap()
        .contains("TODO: Ship fast-rs"));

    let mut bad_req = Request::new(Method::Post, "/tasks");
    bad_req.insert_state(config);
    bad_req.body = b"{broken json".to_vec();
    let bad_res = (route.handler)(bad_req).await;
    assert_eq!(bad_res.status, StatusCode::UnprocessableEntity);
}

// Verifies #[delete] sets Method::Delete and generates RouteMeta for /openapi.json.
#[tokio::test]
async fn delete_macro_builds_route_meta() {
    let route = delete_task();
    let meta = route.meta();
    assert_eq!(meta.method, Method::Delete);
    assert_eq!(meta.path, "/tasks/{id}");

    let mut req = Request::new(Method::Delete, "/tasks/7");
    req.path_params.insert("id".into(), "7".into());
    let res = (route.handler)(req).await;
    assert_eq!(res.status, StatusCode::NoContent);
}

// Verifies all four crates work end-to-end through App::register and App::dispatch.
#[tokio::test]
async fn app_registers_macro_routes_and_dispatches_end_to_end() {
    let mut app = App::new();
    app.insert_state(Arc::new(AppConfig {
        prefix: "TODO".into(),
    }))
    .register(ping())
    .register(get_task())
    .register(create_task());

    let get_res = app.dispatch(Request::new(Method::Get, "/tasks/42")).await;
    assert_eq!(get_res.status, StatusCode::Ok);
    assert!(String::from_utf8(get_res.body)
        .unwrap()
        .contains("Task #42"));

    let mut post_req = Request::new(Method::Post, "/tasks");
    post_req.body = br#"{"title":"Wire all crates"}"#.to_vec();
    let post_res = app.dispatch(post_req).await;
    assert_eq!(post_res.status, StatusCode::Created);
    assert!(String::from_utf8(post_res.body)
        .unwrap()
        .contains("TODO: Wire all crates"));

    let docs_res = app
        .dispatch(Request::new(Method::Get, "/openapi.json"))
        .await;
    assert_eq!(docs_res.status, StatusCode::Ok);
    let docs_body = String::from_utf8(docs_res.body).unwrap();
    assert!(docs_body.contains("\"/tasks/{id}\""));
    assert!(docs_body.contains("\"create_task\""));
}
