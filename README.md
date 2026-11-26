# fast-rs

![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![Runtime](https://img.shields.io/badge/Async-Tokio-blue)
![Inspired By](https://img.shields.io/badge/Inspired%20By-FastAPI-009688?logo=fastapi)

A compile-time, FastAPI-inspired async web micro-framework built in Rust.

**fast-rs** reimagines Python FastAPI's core ergonomics—decorator-style routing, typed request extraction, structured HTTP error handling, and automatic `/docs` route discovery—by shifting runtime reflection and validation to **Rust's compile-time type system, trait solver, and procedural macros**.

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

If a handler requests an invalid extractor or returns a type that cannot be converted into an HTTP response, `cargo check` rejects it before the server ever boots.

---

## Advanced Rust Features Demonstrated

| Rust Concept                               | Crate                 | Implementation                                                                                                                           |
| :----------------------------------------- | :-------------------- | :--------------------------------------------------------------------------------------------------------------------------------------- |
| **Procedural Attribute Macros**            | `crates/fast-macros`  | `#[get]`, `#[post]`, and `#[delete]` built with `syn`, `quote`, and `proc-macro2` to mirror Python route decorators.                     |
| **Declarative Macros (`macro_rules!`)**    | `crates/fast-extract` | Generates variadic `Handler` trait implementations for async functions taking 0 to 3 typed extractors.                                   |
| **Traits, Generics & Type Erasure**        | `crates/fast-extract` | `FromRequest` / `IntoResponse` traits, `TypeId`-keyed `Any` state map, and `Pin<Box<dyn Future>>` type-erased handlers (`BoxedHandler`). |
| **Lifetimes & Zero-Copy Parsing**          | `crates/fast-core`    | Borrows HTTP method, path, and header slices directly from raw TCP buffers using `httparse` without unnecessary copies.                  |
| **Async Concurrency & Shared State**       | `crates/fast-core`    | Non-blocking `tokio::net::TcpListener` spawning concurrent tasks with `Arc<RwLock<T>>` shared state.                                     |
| **Algebraic Data Types & `?` Propagation** | `crates/fast-http`    | `ApiError` enum implementing `From` conversions so handlers propagate JSON/parse failures cleanly with `?`.                              |

---

## Request Lifecycle

```text
TCP Stream (tokio)
   │
   ▼
Zero-Copy HTTP/1.1 Parser (fast-core::parser + httparse)
   │  └─ builds Request { method, path, headers, query, body, extensions }
   ▼
Slice-Based Route Matcher (fast-core::router)
   │  └─ extracts /tasks/{id} path_params & injects shared State<T>
   ▼
Type-Erased BoxedHandler (fast-extract::handler)
   │  └─ runs FromRequest for Path<T>, Json<T>, State<T> (short-circuits to ApiError on failure)
   ▼
Async Route Handler -> IntoResponse -> Raw HTTP/1.1 Bytes -> TCP Write
```

---

## Workspace Structure

```text
fast-rs/
├── Cargo.toml                  # Workspace manifest & root facade package (fast-rs)
├── src/
│   └── lib.rs                  # Public re-exports and `fast_rs::prelude`
├── crates/
│   ├── fast-http/              # Core HTTP types (Request, Response, Method, StatusCode), ApiError, /docs manifest
│   ├── fast-extract/           # FromRequest, IntoResponse, Path<T>, Json<T>, State<T>, Handler traits
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

### Build & Test

```bash
git clone [https://github.com/Furina-star/fast-rs.git](https://github.com/Furina-star/fast-rs.git)
cd fast-rs

# Type-check all workspace crates
cargo check --workspace

# Run unit and integration tests
cargo test --workspace

# Start the demo API (use --release for accurate performance)
cargo run --release -p demo_api
```

### Live API Verification (`curl`)

With `demo_api` running on `http://127.0.0.1:8080`:

```bash
# 1. Inspect auto-generated route manifest
curl -i [http://127.0.0.1:8080/docs](http://127.0.0.1:8080/docs)

# 2. Create a task (201 Created)
curl -i -X POST [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks) \
  -H "Content-Type: application/json" \
  -d '{"title": "Build fast-rs"}'

# 3. Fetch all tasks / single task (200 OK)
curl -i [http://127.0.0.1:8080/tasks](http://127.0.0.1:8080/tasks)
curl -i [http://127.0.0.1:8080/tasks/1](http://127.0.0.1:8080/tasks/1)

# 4. Trigger automatic ApiError responses (422, 400, 404)
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

---

## Team & Branch Workflow

This repository uses trunk-based development with strict crate ownership per branch:

| Branch                | Scope                                           | Crate / Folder Ownership                         |
| :-------------------- | :---------------------------------------------- | :----------------------------------------------- |
| `main`                | Protected integration branch                    | Root `Cargo.toml`, `src/lib.rs`, frozen traits   |
| `feat/core-runtime`   | Async server, zero-copy parser & router         | `crates/fast-core/`                              |
| `feat/macros-extract` | Attribute proc-macros & variadic handlers       | `crates/fast-macros/`, `crates/fast-extract/`    |
| `feat/http-docs`      | HTTP types, query parsing, `ApiError` & `/docs` | `crates/fast-http/`                              |
| `feat/demo-bench`     | Demo CRUD API, FastAPI baseline & benchmarks    | `examples/demo_api/`, `benchmarks/`, `README.md` |

### Contributing Rules

1. Never push directly to `main`; open a Pull Request from your assigned branch.
2. Sync with `main` before starting new work: `git pull origin main --rebase`.
3. Verify `cargo check --workspace` and `cargo test --workspace` pass before requesting review.

---

## Credits & Inspiration

- Inspired by [**FastAPI**](https://fastapi.tiangolo.com/) for its ergonomic decorator routing, typed validation, and automatic `/docs` experience.
- Extractor and type-erased handler architecture inspired by [**Axum**](https://github.com/tokio-rs/axum) and the Rust async ecosystem.
