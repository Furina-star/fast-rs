//! Async Tokio TCP server and connection dispatch loop.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fast_http::{ApiError, Request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::time::timeout;

use crate::parser::{parse_request, MAX_REQUEST_BYTES};
use crate::router::App;

const READ_DEADLINE: Duration = Duration::from_secs(10);
const CLOSE_DRAIN_DEADLINE: Duration = Duration::from_millis(250);

impl App {
    /// Listens for HTTP requests and dispatches them on concurrent connection tasks.
    pub async fn run(self, addr: impl ToSocketAddrs) -> io::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        serve(listener, Arc::new(self)).await
    }
}

async fn serve(listener: TcpListener, app: Arc<App>) -> io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, app).await {
                eprintln!("Connection {peer}: {error}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, app: Arc<App>) -> io::Result<()> {
    let started = Instant::now();
    let Ok(read) = timeout(READ_DEADLINE, read_request(&mut stream)).await else {
        return Ok(());
    };
    let response = match read? {
        Ok(None) => return Ok(()),
        Err(error) => error.to_response(),
        Ok(Some(request)) => {
            let method = request.method;
            let path = request.path.escape_debug().to_string();
            let response = app.dispatch(request).await;
            eprintln!(
                "{method} {path} {} {:.2?}",
                response.status.as_u16(),
                started.elapsed()
            );
            response
        }
    };
    stream.write_all(&response.to_http_bytes()).await?;
    stream.shutdown().await?;

    // A reset caused by unread input can hide the response from the client.
    let _ = timeout(
        CLOSE_DRAIN_DEADLINE,
        tokio::io::copy(
            &mut stream.take(MAX_REQUEST_BYTES as u64),
            &mut tokio::io::sink(),
        ),
    )
    .await;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> io::Result<Result<Option<Request>, ApiError>> {
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];
    while buf.len() < MAX_REQUEST_BYTES {
        let read_limit = chunk.len().min(MAX_REQUEST_BYTES - buf.len());
        let read = stream.read(&mut chunk[..read_limit]).await?;
        if read == 0 {
            return Ok(if buf.is_empty() {
                Ok(None)
            } else {
                Err(ApiError::BadRequest("Incomplete HTTP request".into()))
            });
        }
        buf.extend_from_slice(&chunk[..read]);
        match parse_request(&buf) {
            Ok(None) => continue,
            parsed => return Ok(parsed),
        }
    }
    Ok(Err(ApiError::BadRequest(
        "Request exceeds size limit".into(),
    )))
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::time::Duration;

    use fast_http::{Method, Response, RouteMeta, StatusCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    use super::{handle_connection, serve};
    use crate::router::App;

    #[tokio::test]
    async fn fragmented_request_and_concurrent_client_complete() {
        let state = Arc::new(42_u32);
        let mut app = App::new();
        app.insert_state(Arc::clone(&state));
        for method in [Method::Get, Method::Post] {
            let expected = Arc::clone(&state);
            app.route(
                RouteMeta {
                    method,
                    path: "/tasks".into(),
                    handler_name: "tasks".into(),
                },
                Arc::new(move |req| {
                    assert!(Arc::ptr_eq(&req.get_state().unwrap(), &expected));
                    Box::pin(async { Response::new(StatusCode::Ok, req.body) })
                }),
            );
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, Arc::new(app)));
        let result = timeout(Duration::from_secs(5), async {
            let mut slow = TcpStream::connect(addr).await?;
            slow.write_all(b"POST /tasks HTTP/1.1\r\nContent-Len")
                .await?;
            tokio::task::yield_now().await;
            slow.write_all(b"gth: 3\r\n\r\nab").await?;

            let mut fast = TcpStream::connect(addr).await?;
            fast.write_all(b"GET /tasks HTTP/1.1\r\n\r\n").await?;
            let mut fast_response = Vec::new();
            fast.read_to_end(&mut fast_response).await?;

            slow.write_all(b"c").await?;
            let mut slow_response = Vec::new();
            slow.read_to_end(&mut slow_response).await?;
            Ok::<_, io::Error>((fast_response, slow_response))
        })
        .await;
        server.abort();
        let (fast_response, slow_response) = result.unwrap().unwrap();
        assert!(fast_response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(slow_response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(slow_response.ends_with(b"\r\n\r\nabc"));
    }

    #[tokio::test]
    async fn truncated_request_returns_400() {
        let response = timeout(Duration::from_secs(5), async {
            let (mut client, server) = connection_pair().await?;
            let connection = tokio::spawn(handle_connection(server, Arc::new(App::new())));
            client
                .write_all(b"POST /tasks HTTP/1.1\r\nContent-Length: 3\r\n\r\nab")
                .await?;
            client.shutdown().await?;
            let mut response = Vec::new();
            client.read_to_end(&mut response).await?;
            connection.await.unwrap()?;
            Ok::<_, io::Error>(response)
        })
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
    }

    #[tokio::test]
    async fn early_rejection_returns_400_with_unread_input() {
        let response = timeout(Duration::from_secs(5), async {
            let (mut client, server) = connection_pair().await?;
            let mut request =
                b"POST /tasks HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1048577\r\n\r\n"
                    .to_vec();
            request.resize(32 * 1024, b'a');
            client.write_all(&request).await?;

            let connection = tokio::spawn(handle_connection(server, Arc::new(App::new())));
            let mut response = Vec::new();
            client.read_to_end(&mut response).await?;
            client.shutdown().await?;
            connection.await.unwrap()?;
            Ok::<_, io::Error>(response)
        })
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
        assert!(response.ends_with(b"{\"detail\":\"Request body exceeds size limit\"}"));
    }

    #[tokio::test]
    async fn stalled_request_is_closed_after_read_deadline() {
        let response = timeout(Duration::from_secs(15), async {
            let (mut client, server) = connection_pair().await?;
            client.write_all(b"GET /tasks HTTP/1.1\r\nHost:").await?;
            let started = tokio::time::Instant::now();
            let connection = tokio::spawn(handle_connection(server, Arc::new(App::new())));
            let mut response = Vec::new();
            client.read_to_end(&mut response).await?;
            connection.await.unwrap()?;
            assert!(started.elapsed() >= Duration::from_secs(10));
            Ok::<_, io::Error>(response)
        })
        .await
        .unwrap()
        .unwrap();
        assert!(response.is_empty());
    }

    async fn connection_pair() -> io::Result<(TcpStream, TcpStream)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let (client, (server, _)) = tokio::try_join!(
            TcpStream::connect(listener.local_addr()?),
            listener.accept()
        )?;
        Ok((client, server))
    }
}
