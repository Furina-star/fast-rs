//! Zero-copy HTTP/1.1 request parser built on httparse over raw TCP buffers.
use std::str;

use fast_http::{ApiError, Method, Request};

const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_HEADERS: usize = 64;
pub(crate) const MAX_REQUEST_BYTES: usize = MAX_HEAD_BYTES + 1024 * 1024;
const MAX_BODY_BYTES: usize = MAX_REQUEST_BYTES - MAX_HEAD_BYTES;

struct BorrowedRequest<'head, 'a> {
    method: Method,
    path: &'a str,
    query: &'a str,
    headers: &'head [httparse::Header<'a>],
    body: &'a [u8],
}

pub(crate) fn parse_request(buf: &[u8]) -> Result<Option<Request>, ApiError> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let Some(parsed) = parse_borrowed(buf, &mut headers)? else {
        return Ok(None);
    };

    let mut request = Request::new(parsed.method, parsed.path);
    // shortcut: decode %20 locally until fast-http exposes its query helper.
    request.query = parsed
        .query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.replace("%20", " "), value.replace("%20", " ")))
        .collect();
    for header in parsed.headers {
        let value =
            str::from_utf8(header.value).map_err(|_| bad_request("Invalid header value"))?;
        request
            .headers
            .insert(header.name.to_ascii_lowercase(), value.to_owned());
    }
    request.body = parsed.body.to_vec();
    Ok(Some(request))
}

fn parse_borrowed<'head, 'a>(
    buf: &'a [u8],
    headers: &'head mut [httparse::Header<'a>],
) -> Result<Option<BorrowedRequest<'head, 'a>>, ApiError> {
    let mut head = httparse::Request::new(headers);
    let head_buf = &buf[..buf.len().min(MAX_HEAD_BYTES)];
    let head_len = match head
        .parse(head_buf)
        .map_err(|_| bad_request("Invalid HTTP head"))?
    {
        httparse::Status::Complete(len) => len,
        httparse::Status::Partial if buf.len() < MAX_HEAD_BYTES => return Ok(None),
        httparse::Status::Partial => return Err(bad_request("Request head exceeds size limit")),
    };

    let method = head
        .method
        .ok_or_else(|| bad_request("Missing HTTP method"))?
        .parse()
        .map_err(ApiError::BadRequest)?;
    let target = head
        .path
        .ok_or_else(|| bad_request("Missing request target"))?;
    if !target.starts_with('/') || target.contains('#') {
        return Err(bad_request("Expected an origin-form request target"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let mut body_len = None;
    for header in head.headers.iter() {
        let value =
            str::from_utf8(header.value).map_err(|_| bad_request("Invalid header value"))?;
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(bad_request("Transfer-Encoding is unsupported"));
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if body_len.is_some() {
                return Err(bad_request("Duplicate Content-Length"));
            }
            let digits = value.trim_matches([' ', '\t']);
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(bad_request("Invalid Content-Length"));
            }
            let len = digits
                .parse::<usize>()
                .map_err(|_| bad_request("Invalid Content-Length"))?;
            if len > MAX_BODY_BYTES {
                return Err(bad_request("Request body exceeds size limit"));
            }
            body_len = Some(len);
        }
    }

    let Some(body) = buf.get(head_len..head_len + body_len.unwrap_or(0)) else {
        return Ok(None);
    };
    Ok(Some(BorrowedRequest {
        method,
        path,
        query,
        headers: head.headers,
        body,
    }))
}

fn bad_request(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}

#[cfg(test)]
mod tests {
    use fast_http::{Method, StatusCode};

    use super::parse_request;

    #[test]
    fn parses_request_fields_and_exact_body() {
        let request = parse_request(
            b"POST /tasks?q=hello%20world HTTP/1.1\r\nHost: localhost\r\ncOnTeNt-LeNgTh:\t3 \t\r\n\r\nabcTAIL",
        )
        .unwrap()
        .unwrap();
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.path, "/tasks");
        assert_eq!(
            request.query.get("q").map(String::as_str),
            Some("hello world")
        );
        assert_eq!(
            request.headers.get("host").map(String::as_str),
            Some("localhost")
        );
        assert_eq!(request.body, b"abc");

        let request = parse_request(b"GET /tasks HTTP/1.0\r\n\r\n")
            .unwrap()
            .unwrap();
        assert!(request.body.is_empty());
    }

    #[test]
    fn partial_request_waits() {
        for input in [
            b"GET /tasks HTTP/1.1\r\nHost: local".as_slice(),
            b"POST /tasks HTTP/1.1\r\nContent-Length: 3\r\n\r\nab",
        ] {
            assert!(parse_request(input).unwrap().is_none());
        }
    }

    fn assert_bad_request(input: &[u8]) {
        assert_eq!(
            parse_request(input).unwrap_err().status_code(),
            StatusCode::BadRequest
        );
    }

    #[test]
    fn rejects_invalid_framing() {
        for input in [
            b"GET /tasks\r\n\r\n".as_slice(),
            b"GET http://localhost/tasks HTTP/1.1\r\n\r\n",
            b"GET /tasks#fragment HTTP/1.1\r\n\r\n",
            b"GET /tasks HTTP/1.1\r\nX-Value: \xff\r\n\r\n",
            b"TRACE /tasks HTTP/1.1\r\n\r\n",
            b"POST /tasks HTTP/1.1\r\ntRaNsFeR-EnCoDiNg: identity\r\n\r\n",
            b"POST /tasks HTTP/1.1\r\nContent-Length: 1\r\ncontent-length: 1\r\n\r\na",
            b"POST /tasks HTTP/1.1\r\nContent-Length: +1\r\n\r\na",
            b"POST /tasks HTTP/1.1\r\nContent-Length: -1\r\n\r\na",
            b"POST /tasks HTTP/1.1\r\nContent-Length: 1, 1\r\n\r\na",
            b"POST /tasks HTTP/1.1\r\nContent-Length: \t\r\n\r\n",
        ] {
            assert_bad_request(input);
        }
    }

    #[test]
    fn bounds_head_header_count_and_body() {
        let mut head = b"POST /tasks HTTP/1.1\r\nContent-Length: 1048576\r\n".to_vec();
        for index in 0..62 {
            head.extend_from_slice(format!("X-{index}: value\r\n").as_bytes());
        }
        head.extend_from_slice(b"X-Fill: ");
        head.resize(8192 - 4, b'a');

        let mut oversized = head.clone();
        oversized.extend_from_slice(b"a\r\n\r\n");
        assert_bad_request(&oversized);
        head.extend_from_slice(b"\r\n\r\n");
        assert_eq!(head.len(), 8192);
        assert!(parse_request(&head).unwrap().is_none());
        head.resize(8192 + 1048576, b'b');
        assert_eq!(
            parse_request(&head).unwrap().unwrap().body,
            vec![b'b'; 1048576]
        );

        let mut unterminated = b"GET / HTTP/1.1\r\nX-Fill: ".to_vec();
        unterminated.resize(8192, b'a');
        assert_bad_request(&unterminated);

        let excessive_headers = format!("GET / HTTP/1.1\r\n{}\r\n", "X-Test: a\r\n".repeat(65));
        assert_bad_request(excessive_headers.as_bytes());
        assert_bad_request(b"POST / HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n");
        assert_bad_request(b"POST / HTTP/1.1\r\nContent-Length: 999999999999999999999999\r\n\r\n");
    }
}
