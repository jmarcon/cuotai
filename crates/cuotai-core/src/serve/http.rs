//! The HTTP/1.x subset the local server speaks: one `GET` per connection,
//! parsed with `httparse`, answered with JSON and closed.

use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Bytes read per call while waiting for the end of the head.
const CHUNK: usize = 1024;
/// What is read and discarded after answering, so an unread request body
/// does not turn the close into a reset that loses the answer.
const LINGER_BYTES: usize = 64 * 1024;
const LINGER_TIME: Duration = Duration::from_millis(500);

/// The request line and headers of one request.
pub(super) struct Head {
    pub method: String,
    /// As sent: path and query.
    pub target: String,
    /// Lowercase names and raw values.
    headers: Vec<(String, Vec<u8>)>,
}

impl Head {
    /// Every value of header `name` (lowercase).
    pub fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.headers
            .iter()
            .filter(move |(key, _)| key == name)
            .map(|(_, value)| value.as_slice())
    }

    /// RF3.2: a browser marks requests a page makes with `Origin` (CORS and
    /// writes) and `Sec-Fetch-Site` (every request; `none` only for what the
    /// user typed or bookmarked).
    pub fn sent_by_page(&self) -> bool {
        self.values("origin").next().is_some()
            || self
                .values("sec-fetch-site")
                .any(|site| !site.eq_ignore_ascii_case(b"none"))
    }

    /// The credentials of a single `Authorization: Bearer` header.
    pub fn bearer(&self) -> Option<&[u8]> {
        let mut values = self.values("authorization");
        let (Some(value), None) = (values.next(), values.next()) else {
            return None;
        };
        let (scheme, rest) = value.split_at_checked(7)?;
        scheme
            .eq_ignore_ascii_case(b"bearer ")
            .then(|| rest.trim_ascii())
    }
}

pub(super) enum Read {
    Head(Head),
    /// The peer left (or never sent a whole head): nothing to answer.
    Closed,
    TooLarge,
    Malformed,
}

pub(super) async fn read_head(
    stream: &mut TcpStream,
    max_bytes: usize,
    max_headers: usize,
) -> Read {
    let mut buf = Vec::with_capacity(CHUNK);
    let mut chunk = [0u8; CHUNK];
    loop {
        let want = (max_bytes - buf.len()).min(CHUNK);
        if want == 0 {
            return Read::TooLarge;
        }
        match stream.read(&mut chunk[..want]).await {
            Ok(0) | Err(_) => return Read::Closed,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        let mut headers = vec![httparse::EMPTY_HEADER; max_headers];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buf) {
            Ok(httparse::Status::Complete(_)) => {
                return Read::Head(Head {
                    method: request.method.unwrap_or_default().to_owned(),
                    target: request.path.unwrap_or_default().to_owned(),
                    headers: request
                        .headers
                        .iter()
                        .map(|h| (h.name.to_ascii_lowercase(), h.value.to_vec()))
                        .collect(),
                })
            }
            Ok(httparse::Status::Partial) => {}
            Err(httparse::Error::TooManyHeaders) => return Read::TooLarge,
            Err(_) => return Read::Malformed,
        }
    }
}

pub(super) struct Response {
    pub status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, value: &impl Serialize) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self {
                status,
                headers: Vec::new(),
                body,
            },
            Err(_) => Self::error(500, "internal"),
        }
    }

    /// `{"error": code}`.
    pub fn error(status: u16, code: &'static str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: format!("{{\"error\":\"{code}\"}}").into_bytes(),
        }
    }

    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        414 => "URI Too Long",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

/// RF2.4: every answer is JSON, never cached and closes the connection.
/// `include_body` is false for `HEAD`, which gets the headers only.
pub(super) async fn write(
    stream: &mut TcpStream,
    response: Response,
    include_body: bool,
) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason(response.status),
        response.body.len()
    );
    for (name, value) in &response.headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    if include_body {
        bytes.extend_from_slice(&response.body);
    }
    stream.write_all(&bytes).await?;
    stream.flush().await
}

/// Close our side, then drain (bounded) until the peer closes too.
pub(super) async fn linger(stream: &mut TcpStream) {
    if stream.shutdown().await.is_err() {
        return;
    }
    let mut chunk = [0u8; CHUNK];
    let mut left = LINGER_BYTES;
    let _ = tokio::time::timeout(LINGER_TIME, async {
        while left > 0 {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => left = left.saturating_sub(n),
            }
        }
    })
    .await;
}

/// A percent-encoded path segment, decoded as UTF-8.
pub(super) fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}
