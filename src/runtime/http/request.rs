//! Bounded HTTP request parsing.
//!
//! This layer accepts only a deliberately small, validated subset of HTTP/1.x
//! before application routing receives the request.

use std::{
    io::{BufRead, BufReader, Read},
    net::TcpStream,
};

use super::{
    MAX_REQUEST_BYTES, MAX_REQUEST_HEADER_BYTES, MAX_REQUEST_HEADERS, MAX_REQUEST_LINE_BYTES,
};

/// One parsed HTTP request, including whether its client will close the connection.
pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: String,
    pub(super) close_connection: bool,
}

/// A request parsing failure that can be represented safely to an HTTP client.
pub(super) struct RequestReadError {
    pub(super) status: u16,
    pub(super) code: &'static str,
    pub(super) message: String,
}

impl RequestReadError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "ORNI_HTTP_INVALID_REQUEST",
            message: message.into(),
        }
    }

    pub(super) fn headers_too_large(message: impl Into<String>) -> Self {
        Self {
            status: 431,
            code: "ORNI_HTTP_HEADERS_TOO_LARGE",
            message: message.into(),
        }
    }
}

/// Reads a bounded HTTP/1.1 request, including a JSON body when present.
pub(super) fn read_request(
    reader: &mut BufReader<TcpStream>,
) -> Result<Option<HttpRequest>, RequestReadError> {
    let Some(request_line) = read_bounded_line(reader, MAX_REQUEST_LINE_BYTES)? else {
        return Ok(None);
    };
    let mut content_length = 0_usize;
    let mut close_connection = false;
    let mut header_bytes = request_line.len();
    let mut header_count = 0_usize;
    let mut is_http_1_0 = false;
    loop {
        let header = read_bounded_line(reader, MAX_REQUEST_LINE_BYTES)?.ok_or_else(|| {
            RequestReadError::invalid("request ended before the header section was complete")
        })?;
        header_bytes = header_bytes.checked_add(header.len()).ok_or_else(|| {
            RequestReadError::headers_too_large("request headers exceed the configured limit")
        })?;
        if header_bytes > MAX_REQUEST_HEADER_BYTES {
            return Err(RequestReadError::headers_too_large(format!(
                "request headers exceed the {MAX_REQUEST_HEADER_BYTES}-byte limit"
            )));
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        header_count += 1;
        if header_count > MAX_REQUEST_HEADERS {
            return Err(RequestReadError::headers_too_large(format!(
                "request has more than the {MAX_REQUEST_HEADERS}-header limit"
            )));
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(RequestReadError::invalid("malformed HTTP request header"));
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| RequestReadError::invalid("invalid Content-Length header"))?;
        }
        if name.eq_ignore_ascii_case("connection") && value.trim().eq_ignore_ascii_case("close") {
            close_connection = true;
        }
    }
    if content_length > MAX_REQUEST_BYTES {
        return Err(RequestReadError::invalid(format!(
            "request body exceeds the {MAX_REQUEST_BYTES}-byte limit"
        )));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).map_err(|_| {
        RequestReadError::invalid("request body ended before Content-Length bytes were received")
    })?;
    let body = String::from_utf8(body)
        .map_err(|_| RequestReadError::invalid("request body must be valid UTF-8"))?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| RequestReadError::invalid("malformed HTTP request line"))?
        .to_owned();
    let path = parts
        .next()
        .ok_or_else(|| RequestReadError::invalid("malformed HTTP request line"))?
        .to_owned();
    match parts.next() {
        Some("HTTP/1.1") => {}
        Some("HTTP/1.0") => is_http_1_0 = true,
        _ => {
            return Err(RequestReadError::invalid(
                "unsupported or malformed HTTP version",
            ));
        }
    }
    if parts.next().is_some() {
        return Err(RequestReadError::invalid("malformed HTTP request line"));
    }
    Ok(Some(HttpRequest {
        method,
        path,
        body,
        close_connection: close_connection || is_http_1_0,
    }))
}

/// Reads one UTF-8 HTTP line without allowing an unbounded allocation.
fn read_bounded_line(
    reader: &mut BufReader<TcpStream>,
    maximum_bytes: usize,
) -> Result<Option<String>, RequestReadError> {
    let mut line = Vec::with_capacity(128);
    loop {
        let (consume, complete) = {
            let available = reader.fill_buf().map_err(|error| {
                RequestReadError::invalid(format!("could not read request: {error}"))
            })?;
            if available.is_empty() {
                if line.is_empty() {
                    return Ok(None);
                }
                return Err(RequestReadError::invalid(
                    "request ended before the line was complete",
                ));
            }
            let consume = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            (
                consume,
                consume <= available.len() && available[consume - 1] == b'\n',
            )
        };
        let next_length = line.len().checked_add(consume).ok_or_else(|| {
            RequestReadError::headers_too_large("request line exceeds the configured limit")
        })?;
        if next_length > maximum_bytes {
            return Err(RequestReadError::headers_too_large(format!(
                "request line exceeds the {maximum_bytes}-byte limit"
            )));
        }
        let available = reader.fill_buf().map_err(|error| {
            RequestReadError::invalid(format!("could not read request: {error}"))
        })?;
        line.extend_from_slice(&available[..consume]);
        reader.consume(consume);
        if complete {
            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| RequestReadError::invalid("request headers must be valid UTF-8"));
        }
    }
}
