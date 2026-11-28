<div align="center">

# fast-rs

![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![Runtime](https://img.shields.io/badge/Async-Tokio-blue)
![Inspired By](https://img.shields.io/badge/Inspired%20By-FastAPI-009688?logo=fastapi)
![License](https://img.shields.io/badge/License-MIT-green)

</div>

A compile-time, FastAPI-inspired async web micro-framework built in Rust.

**fast-rs** reimagines Python FastAPI's core ergonomics—decorator-style routing, typed request extraction, structured HTTP error handling, and automatic interactive `/docs`—by shifting runtime reflection and validation to **Rust's compile-time type system, trait solver, and procedural macros**.

---

## FastAPI vs. `fast-rs`

Where Python inspects type hints and decorators at **runtime**, `fast-rs` resolves routes, extractors, and response types at **compile time**:

```python
# Python (FastAPI) — validated at runtime
@app.post("/tasks", status_code=201)
async def create_task(payload: CreateTask) -> Task:
    ...

@app.get("/tasks/{id}")
async def get_task(id: int) -> Task:
    raise HTTPException(status_code=404, detail="Task not found")
```

```rust
// Rust (fast-rs) — verified at compile time
#[post("/tasks")]
async fn create_task(
    State(db): State<RwLock<Vec<Task>>>,
    Json(payload): Json<CreateTask>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    ...
}

#[get("/tasks/{id}")]
async fn get_task(
    State(db): State<RwLock<Vec<Task>>>,
    Path(id): Path<u32>,
) -> Result<Json<Task>, ApiError> {
    Err(ApiError::NotFound(format!("Task {id} not found")))
}
```

If a handler omits `async`, requests an invalid extractor, or returns a type that cannot convert into an HTTP response, `cargo check` rejects it before the server ever boots.

---

## Quickstart

Import the prelude, decorate `async fn` handlers with `#[get]`, `#[post]`, or `#[delete]`, and register them on `App`:

```rust
use std::sync::{Arc, RwLock};
use fast_rs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
struct Task {
    id: u32,
    title: String,
}

#[derive(Deserialize)]
struct CreateTask {
    title: String,
}

#[get("/tasks")]
async fn list_tasks(State(db): State<RwLock<Vec<Task>>>) -> Json<Vec<Task>> {
    let tasks = db.read().unwrap().clone();
    Json(tasks)
}

#[post("/tasks")]
async fn create_task(
    State(db): State<RwLock<Vec<Task>>>,
    Json(payload): Json<CreateTask>,
) -> (StatusCode, Json<Task>) {
    let mut tasks = db.write().unwrap();
    let task = Task {
        id: (tasks.len() as u32) + 1,
        title: payload.title,
    };
    tasks.push(task.clone());
    (StatusCode::Created, Json(task))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db = Arc::new(RwLock::new(Vec::<Task>::new()));

    let mut app = App::new();
    app.insert_state(db)
        .register(list_tasks())
        .register(create_task());

    app.run("127.0.0.1:8080").await
}
```

Every `App` automatically serves:

- **`GET /docs`** — Interactive Swagger UI
- **`GET /openapi.json`** — Auto-generated OpenAPI 3.0 specification

---

## Advanced Rust Features Demonstrated

| Rust Concept                               | Crate                 | Implementation                                                                                                                                                    |
| :----------------------------------------- | :-------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Procedural Attribute Macros**            | `crates/fast-macros`  | `#[get]`, `#[post]`, and `#[delete]` built with `syn`, `quote`, and `proc-macro2` to compile handlers into `Route` factories with compile-time `async fn` checks. |
| **Declarative Macros (`macro_rules!`)**    | `crates/fast-extract` | `impl_handler!` generates variadic `Handler` trait implementations for async functions taking 1 to 3 typed extractors.                                            |
| **Traits, Generics & Type Erasure**        | `crates/fast-extract` | `FromRequest` / `IntoResponse` traits, `TypeId`-keyed `Any` state map, and `Pin<Box<dyn Future>>` type-erased handlers (`BoxedHandler`).                          |
| **Lifetimes & Zero-Copy Parsing**          | `crates/fast-core`    | Borrows HTTP method, path, query, header, and body slices directly from raw TCP buffers using `httparse` (`BorrowedRequest<'head, 'a>`).                          |
| **Async Concurrency & Shared State**       | `crates/fast-core`    | Non-blocking `tokio::net::TcpListener` spawning concurrent connection tasks with read/write deadlines and `Arc`-backed shared state.                              |
| **Algebraic Data Types & `?` Propagation** | `crates/fast-http`    | `ApiError` enum with `From` conversions so handlers propagate JSON and integer parse errors cleanly into CORS-enabled JSON responses.                             |

---

## Request Lifecycle

```text
TCP Stream (tokio)
   │
   ▼
Zero-Copy HTTP/1.1 Parser (fast-core::parser + httparse)
   │  └─ validates framing/size limits & builds Request { method, path, headers, query, body }
   ▼
Slice-Based Route Matcher (fast-core::router)
   │  └─ serves /docs & /openapi.json, matches literal/dynamic paths, captures {params}, injects State<T>
   ▼
Type-Erased BoxedHandler (fast-extract::handler)
   │  └─ runs FromRequest for Path<T>, Json<T>, State<T> (short-circuits to ApiError on failure)
   ▼
Async Route Handler -> IntoResponse -> Raw HTTP/1.1 Bytes -> TCP Write
```

For an in-depth breakdown of crate boundaries, macro expansion, memory safety guarantees, and engineering trade-offs, see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## Workspace Structure

```text
fast-rs/
├── Cargo.toml                  # Workspace manifest & root facade package (fast-rs)
├── LICENSE                     # MIT License
├── README.md                   # Project overview, quickstart, and benchmarks
├── docs/
│   └── ARCHITECTURE.md         # Internal architecture, crate graph, and design trade-offs
├── src/
│   └── lib.rs                  # Public re-exports (`App`, `Route`, macros) and `fast_rs::prelude`
├── tests/
│   └── macros_test.rs          # End-to-end integration tests across all four crates
├── crates/
│   ├── fast-http/              # Core HTTP types, query parser, CORS, ApiError, OpenAPI 3.0 & Swagger UI
│   ├── fast-extract/           # FromRequest, IntoResponse, Path<T>, Json<T>, State<T>, Handler & Route
│   ├── fast-macros/            # Attribute proc-macros (#[get], #[post], #[delete])
│   └── fast-core/              # Tokio TCP server, zero-copy httparse parser, router, App builder
├── examples/
│   └── demo_api/               # Showcase Task CRUD API built on top of fast-rs
└── benchmarks/
    └── fastapi_ref/            # Equivalent Python FastAPI server and `oha` load-test script
```

---

## Getting Started

### Prerequisites

- **Rust** (stable toolchain with Cargo): `rustc --version`
- **Python 3** with `fastapi` and `uvicorn` _(optional, for running the reference server)_
- **`oha`** _(optional, for load benchmarks)_: `cargo install oha`

### Build, Test & API Docs

```bash
git clone [https://github.com/Furina-star/fast-rs.git](https://github.com/Furina-star/fast-rs.git)
cd fast-rs

# Type-check all workspace crates
cargo check --workspace

# Run all unit and integration tests (31 tests across fast-core, fast-http, and tests/macros_test.rs)
cargo test --workspace

# Build and open generated Rustdoc HTML documentation
cargo doc --workspace --no-deps --open

# Start the demo API (use --release for accurate performance)
cargo run --release -p demo_api
```

### Live API Verification (`curl`)

With `demo_api` running on `http://127.0.0.1:8080`:

```bash
# 1. Inspect auto-generated OpenAPI 3.0 spec & Swagger UI
curl -i [http://127.0.0.1:8080/openapi.json](http://127.0.0.1:8080/openapi.json)
# Open [http://127.0.0.1:8080/docs](http://127.0.0.1:8080/docs) in your browser for interactive Swagger UI

# 2. Create a task (201 Created)
curl -i -X POST [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks) \
  -H "Content-Type: application/json" \
  -d '{"title": "Build fast-rs"}'

# 3. Fetch all tasks / single task (200 OK)
curl -i [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks)
curl -i [http://127.0.0.1:8080/tasks/1](http://127.0.0.1:8080/tasks/1)

# 4. Delete a task (204 No Content)
curl -i -X DELETE [http://127.0.0.1:8080/tasks/1](http://127.0.0.1:8080/tasks/1)

# 5. Trigger automatic ApiError responses (422, 400, 404)
curl -i -X POST [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks) -H "Content-Type: application/json" -d '{bad json'
curl -i [http://127.0.0.1:8080/tasks/not-a-number](http://127.0.0.1:8080/tasks/not-a-number)
curl -i [http://127.0.0.1:8080/tasks/999](http://127.0.0.1:8080/tasks/999)
```

---

## Benchmarks (`fast-rs` vs. Python FastAPI)

Run both servers locally and execute the side-by-side benchmark (`10,000` requests, `50` concurrent connections):

```bash
# Python FastAPI baseline (port 8000)
uvicorn benchmarks.fastapi_ref.main:app --port 8000
oha -n 10000 -c 50 [http://127.0.0.1:8000/tasks](http://127.0.0.1:8000/tasks)

# Rust fast-rs release build (port 8080)
cargo run --release -p demo_api
oha -n 10000 -c 50 [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks)
```

| Metric (`10,000` reqs, `50` concurrent) | Python FastAPI (`uvicorn`) | Rust `fast-rs` (`--release`) |
| :-------------------------------------- | :------------------------: | :--------------------------: |
| **Requests / sec**                      |         _Pending_          |          _Pending_           |
| **Average Latency**                     |         _Pending_          |          _Pending_           |
| **p99 Latency**                         |         _Pending_          |          _Pending_           |

> **Test Environment:** _Pending (record CPU, OS, Python/uvicorn version, and `rustc --version` when filling in results)._

---

## Team & Branch Workflow

This repository uses trunk-based development with crate ownership per branch:

| Branch                | Scope                                           | Crate / Folder Ownership                           |
| :-------------------- | :---------------------------------------------- | :------------------------------------------------- |
| `main`                | Protected integration branch                    | Root `Cargo.toml`, `src/lib.rs`, `tests/`, `docs/` |
| `feat/core-runtime`   | Async server, zero-copy parser & router         | `crates/fast-core/`                                |
| `feat/macros-extract` | Attribute proc-macros & variadic handlers       | `crates/fast-macros/`, `crates/fast-extract/`      |
| `feat/http-docs`      | HTTP types, query parsing, `ApiError` & `/docs` | `crates/fast-http/`                                |
| `feat/demo-bench`     | Demo CRUD API, FastAPI baseline & benchmarks    | `examples/demo_api/`, `benchmarks/`, `README.md`   |

### Contributing Rules

1. Never push directly to `main`; open a Pull Request from your assigned branch.
2. Sync with `main` before starting new work: `git pull origin main --rebase`.
3. Verify `cargo check --workspace`, `cargo test --workspace`, and `cargo doc --workspace --no-deps` pass cleanly before requesting review.

---

## Credits & License

- Inspired by [**FastAPI**](https://fastapi.tiangolo.com/) for its ergonomic decorator routing, typed validation, and automatic `/docs` experience.
- Extractor and type-erased handler architecture inspired by [**Axum**](https://github.com/tokio-rs/axum) and the Rust async ecosystem.
- Licensed under the [MIT License](LICENSE).
