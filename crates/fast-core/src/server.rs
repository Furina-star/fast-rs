//! Async Tokio TCP server and connection dispatch loop.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fast_http::{ApiError, Request};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::time::{sleep, timeout};

use crate::parser::{parse_request, MAX_REQUEST_BYTES};
use crate::router::App;

const READ_DEADLINE: Duration = Duration::from_secs(10);
const WRITE_DEADLINE: Duration = Duration::from_secs(10);
const CLOSE_DRAIN_DEADLINE: Duration = Duration::from_millis(250);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

// Stable ErrorKind does not expose socket descriptor or buffer exhaustion.
const ACCEPT_RESOURCE_ERRORS: &[i32] = &[
    #[cfg(windows)]
    10024, // WSAEMFILE
    #[cfg(windows)]
    10055, // WSAENOBUFS
    #[cfg(any(
        target_os = "linux",
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
        all(
            target_os = "android",
            not(any(
                target_arch = "mips",
                target_arch = "mips64",
                target_arch = "mips32r6",
                target_arch = "mips64r6",
                target_arch = "sparc",
                target_arch = "sparc64"
            ))
        )
    ))]
    libc::ENOBUFS,
    #[cfg(all(
        unix,
        not(any(target_os = "haiku", target_os = "hurd", target_os = "emscripten"))
    ))]
    libc::ENFILE,
    #[cfg(all(
        unix,
        not(any(target_os = "haiku", target_os = "hurd", target_os = "emscripten"))
    ))]
    libc::EMFILE,
];

impl App {
    /// Listens for HTTP requests and dispatches them on concurrent connection tasks.
    pub async fn run(self, addr: impl ToSocketAddrs) -> io::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        serve(listener, Arc::new(self)).await
    }
}

async fn serve(listener: TcpListener, app: Arc<App>) -> io::Result<()> {
    loop {
        let (stream, peer) = accept_connection(|| listener.accept()).await?;
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, app).await {
                log_line(
                    io::stderr().lock(),
                    format_args!("Connection {peer}: {error}"),
                );
            }
        });
    }
}

async fn accept_connection<T, F>(mut accept: impl FnMut() -> F) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    loop {
        let error = match accept().await {
            Ok(connection) => return Ok(connection),
            Err(error) => error,
        };
        match error.kind() {
            io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset => tokio::task::yield_now().await,
            // Linux accept(2) can report pending TCP errors from the new socket.
            #[cfg(target_os = "linux")]
            _ if matches!(
                error.raw_os_error(),
                Some(
                    libc::ENETDOWN
                        | libc::EPROTO
                        | libc::ENOPROTOOPT
                        | libc::EHOSTDOWN
                        | libc::ENONET
                        | libc::EHOSTUNREACH
                        | libc::EOPNOTSUPP
                        | libc::ENETUNREACH
                )
            ) =>
            {
                tokio::task::yield_now().await;
            }
            kind if kind == io::ErrorKind::OutOfMemory
                || error
                    .raw_os_error()
                    .is_some_and(|code| ACCEPT_RESOURCE_ERRORS.contains(&code)) =>
            {
                sleep(ACCEPT_RETRY_DELAY).await;
            }
            _ => return Err(error),
        }
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
            log_line(
                io::stderr().lock(),
                format_args!(
                    "{method} {path} {} {:.2?}",
                    response.status.as_u16(),
                    started.elapsed()
                ),
            );
            response
        }
    };
    write_response(&mut stream, &response.to_http_bytes()).await?;
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

fn log_line(mut sink: impl io::Write, message: std::fmt::Arguments<'_>) {
    let _ = writeln!(sink, "{message}");
}

async fn write_response(stream: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> io::Result<()> {
    timeout(WRITE_DEADLINE, stream.write_all(bytes))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "response write timed out"))?
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
    use std::future::{poll_fn, ready, Future};
    use std::io;
    use std::sync::Arc;
    use std::task::Poll;
    use std::time::Duration;

    use fast_http::{Method, Response, RouteMeta, StatusCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{timeout, Instant};

    use super::{accept_connection, handle_connection, log_line, serve, write_response};
    use crate::router::App;

    #[tokio::test]
    async fn stalled_response_returns_timed_out_error() {
        let (mut writer, _reader) = tokio::io::duplex(1);
        let started = tokio::time::Instant::now();
        let error = timeout(Duration::from_secs(12), write_response(&mut writer, b"ok"))
            .await
            .expect("response write did not finish by its deadline")
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_secs(10));
    }

    #[tokio::test]
    async fn response_write_preserves_io_error() {
        let (mut writer, reader) = tokio::io::duplex(1);
        drop(reader);

        let error = write_response(&mut writer, b"ok").await.unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn logging_continues_after_sink_rejects_write() {
        #[derive(Default)]
        struct RejectingWriter(bool);

        impl io::Write for RejectingWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                self.0 = true;
                Err(io::ErrorKind::BrokenPipe.into())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = RejectingWriter::default();
        log_line(&mut writer, format_args!("test message"));

        assert!(writer.0);
    }

    #[tokio::test]
    async fn accept_retries_transient_connection_errors() {
        let mut outcomes = [
            Err(io::ErrorKind::Interrupted.into()),
            Err(io::ErrorKind::ConnectionAborted.into()),
            Err(io::ErrorKind::ConnectionReset.into()),
            Ok(42),
        ]
        .into_iter();

        assert_eq!(
            accept_connection(|| ready(outcomes.next().unwrap()))
                .await
                .unwrap(),
            42
        );
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn accept_retries_linux_pending_network_errors() {
        for code in [
            libc::EPROTO,
            libc::ENETDOWN,
            libc::ENOPROTOOPT,
            libc::EHOSTDOWN,
            libc::ENONET,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
            libc::ENETUNREACH,
        ] {
            let mut outcomes = [Err(io::Error::from_raw_os_error(code)), Ok(42)].into_iter();

            assert_eq!(
                accept_connection(|| ready(outcomes.next().unwrap()))
                    .await
                    .unwrap(),
                42
            );
        }
    }

    #[tokio::test]
    async fn accept_waits_before_retrying_resource_exhaustion() {
        let mut errors = vec![io::ErrorKind::OutOfMemory.into()];
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_vendor = "apple",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        errors.extend([libc::ENFILE, libc::EMFILE].map(io::Error::from_raw_os_error));
        #[cfg(windows)]
        errors.extend([10024, 10055].map(io::Error::from_raw_os_error));

        for error in errors {
            let mut outcomes = [Err(error), Ok(42)].into_iter();
            let started = Instant::now();
            let mut accepting =
                std::pin::pin!(accept_connection(|| ready(outcomes.next().unwrap())));
            poll_fn(|cx| {
                assert!(accepting.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            assert_eq!(accepting.await.unwrap(), 42);
            assert!(started.elapsed() >= Duration::from_millis(100));
        }
    }

    #[tokio::test]
    async fn accept_propagates_fatal_errors() {
        let mut outcome = Some(Err::<(), _>(io::ErrorKind::InvalidInput.into()));
        let error = accept_connection(|| ready(outcome.take().unwrap()))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

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
