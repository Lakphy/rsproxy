//! Blocking clients and authentication discovery for the control protocol.
//!
//! Requests select TCP, Unix-domain socket or Windows named-pipe transport from
//! the endpoint prefix while preserving one response/error contract.

use crate::server::{unix_api_path, windows_pipe_path};
use crate::{ControlError, ControlResult};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

mod auth;

pub use auth::{
    api_token_path, prepare_server_api_auth, resolve_client_api_token, validate_api_token,
};

/// Configures the process-global bearer token copied into subsequent client requests.
///
/// The token is stored behind a read/write lock, so setting it is atomic with
/// respect to concurrent requests. Passing `None` removes the authorization header.
pub fn set_api_token(token: Option<String>) {
    *api_token_state()
        .write()
        .expect("API token state lock poisoned") = token;
}

fn api_token_state() -> &'static RwLock<Option<String>> {
    static TOKEN: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    TOKEN.get_or_init(|| RwLock::new(None))
}

fn configured_api_token() -> Option<String> {
    api_token_state()
        .read()
        .expect("API token state lock poisoned")
        .clone()
}

/// Sends one connection-closing HTTP/1.1 request and returns its response body.
///
/// Transport is selected from `api`: `unix:` uses a Unix socket, `pipe:` or
/// `npipe:` uses a Windows named pipe, and all other values are TCP addresses.
/// The configured bearer token is copied when the request is built. TCP and Unix
/// reads have a five-second timeout. Non-2xx responses become
/// [`ControlError::HttpStatus`], while malformed response framing becomes
/// [`ControlError::Protocol`]. Response bytes are converted to a string with
/// invalid UTF-8 replaced lossily.
pub fn api_request(method: &str, api: &str, path: &str, body: &str) -> ControlResult<String> {
    api_request_with_timeout(method, api, path, body, Duration::from_secs(5))
}

/// Sends one control request with a caller-selected response timeout.
///
/// This is intended for bounded long-running operations such as replay. A zero
/// timeout is rejected. Named-pipe requests rely on the server-side operation
/// deadline because the Windows pipe adapter does not expose socket timeouts.
pub fn api_request_with_timeout(
    method: &str,
    api: &str,
    path: &str,
    body: &str,
    timeout: Duration,
) -> ControlResult<String> {
    if timeout.is_zero() {
        return Err(ControlError::InvalidRequest(
            "control response timeout must be greater than zero".to_string(),
        ));
    }
    if let Some(socket_path) = unix_api_path(api) {
        return api_request_unix(method, api, socket_path, path, body, timeout);
    }
    if let Some(pipe_path) = windows_pipe_path(api) {
        return api_request_windows_pipe(method, api, pipe_path, path, body);
    }
    let mut stream = TcpStream::connect(api)
        .map_err(|source| ControlError::io(format!("connect {api}"), source))?;
    stream.set_read_timeout(Some(timeout)).map_err(|source| {
        ControlError::io(format!("configure control read timeout for {api}"), source)
    })?;
    api_request_stream(&mut stream, method, api, path, body)
}

/// Sends a GET and streams non-empty newline-delimited response records.
///
/// The callback receives lines without CR/LF and returning `false` ends the
/// operation successfully. Empty heartbeat lines are ignored. TCP and Unix reads
/// time out after 65 seconds without data; an unexpected EOF is a protocol error.
/// Transport and authentication selection match [`api_request`].
pub fn api_stream_lines(
    api: &str,
    path: &str,
    mut on_line: impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    if let Some(socket_path) = unix_api_path(api) {
        return api_stream_lines_unix(api, socket_path, path, &mut on_line);
    }
    if let Some(pipe_path) = windows_pipe_path(api) {
        return api_stream_lines_windows_pipe(api, pipe_path, path, &mut on_line);
    }
    let mut stream = TcpStream::connect(api)
        .map_err(|source| ControlError::io(format!("connect {api}"), source))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(65)))
        .map_err(|source| {
            ControlError::io(format!("configure control read timeout for {api}"), source)
        })?;
    api_stream_lines_from(&mut stream, api, path, &mut on_line)
}

#[cfg(unix)]
fn api_stream_lines_unix(
    api: &str,
    socket_path: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|source| ControlError::io(format!("connect unix socket {socket_path}"), source))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(65)))
        .map_err(|source| {
            ControlError::io(
                format!("configure Unix control read timeout for {socket_path}"),
                source,
            )
        })?;
    api_stream_lines_from(&mut stream, api, path, on_line)
}

#[cfg(not(unix))]
fn api_stream_lines_unix(
    _api: &str,
    socket_path: &str,
    _path: &str,
    _on_line: &mut impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    Err(ControlError::Unsupported(format!(
        "unix control sockets are not supported on this platform: {socket_path}"
    )))
}

#[cfg(windows)]
fn api_stream_lines_windows_pipe(
    api: &str,
    pipe_path: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    let mut stream = crate::server::NamedPipeStream::connect(pipe_path).map_err(|source| {
        ControlError::io(format!("connect Windows named pipe {pipe_path}"), source)
    })?;
    api_stream_lines_from(&mut stream, api, path, on_line)
}

#[cfg(not(windows))]
fn api_stream_lines_windows_pipe(
    _api: &str,
    pipe_path: &str,
    _path: &str,
    _on_line: &mut impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    Err(ControlError::Unsupported(format!(
        "Windows named pipes are not supported on this platform: {pipe_path}"
    )))
}

fn api_stream_lines_from<S: Read + Write>(
    stream: &mut S,
    api: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> bool,
) -> ControlResult<()> {
    let request = api_request_text("GET", api, path, "", configured_api_token().as_deref());
    stream
        .write_all(request.as_bytes())
        .map_err(|source| ControlError::io(format!("write control request to {api}"), source))?;
    stream
        .flush()
        .map_err(|source| ControlError::io(format!("flush control request to {api}"), source))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .map_err(|source| ControlError::io(format!("read control response from {api}"), source))?
        == 0
    {
        return Err(ControlError::protocol(
            "control API closed before the response head",
        ));
    }
    let status = response_status(&line)?;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|source| {
            ControlError::io(format!("read control response from {api}"), source)
        })? == 0
        {
            return Err(ControlError::protocol(
                "control API closed during the response head",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    if !(200..300).contains(&status) {
        let mut body = String::new();
        reader.read_to_string(&mut body).map_err(|source| {
            ControlError::io(format!("read control error response from {api}"), source)
        })?;
        return Err(ControlError::HttpStatus { status, body });
    }

    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|source| ControlError::io(format!("read control stream from {api}"), source))?
            == 0
        {
            return Err(ControlError::protocol("trace follow stream closed"));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if !line.is_empty() && !on_line(line) {
            return Ok(());
        }
    }
}

#[cfg(unix)]
fn api_request_unix(
    method: &str,
    api: &str,
    socket_path: &str,
    path: &str,
    body: &str,
    timeout: Duration,
) -> ControlResult<String> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|source| ControlError::io(format!("connect unix socket {socket_path}"), source))?;
    stream.set_read_timeout(Some(timeout)).map_err(|source| {
        ControlError::io(
            format!("configure Unix control read timeout for {socket_path}"),
            source,
        )
    })?;
    api_request_stream(&mut stream, method, api, path, body)
}

#[cfg(not(unix))]
fn api_request_unix(
    _method: &str,
    _api: &str,
    socket_path: &str,
    _path: &str,
    _body: &str,
    _timeout: Duration,
) -> ControlResult<String> {
    Err(ControlError::Unsupported(format!(
        "unix control sockets are not supported on this platform: {socket_path}"
    )))
}

#[cfg(windows)]
fn api_request_windows_pipe(
    method: &str,
    api: &str,
    pipe_path: &str,
    path: &str,
    body: &str,
) -> ControlResult<String> {
    let mut stream = crate::server::NamedPipeStream::connect(pipe_path).map_err(|source| {
        ControlError::io(format!("connect Windows named pipe {pipe_path}"), source)
    })?;
    api_request_stream(&mut stream, method, api, path, body)
}

#[cfg(not(windows))]
fn api_request_windows_pipe(
    _method: &str,
    _api: &str,
    pipe_path: &str,
    _path: &str,
    _body: &str,
) -> ControlResult<String> {
    Err(ControlError::Unsupported(format!(
        "Windows named pipes are not supported on this platform: {pipe_path}"
    )))
}

fn api_request_stream<S: Read + Write>(
    stream: &mut S,
    method: &str,
    api: &str,
    path: &str,
    body: &str,
) -> ControlResult<String> {
    let token = configured_api_token();
    let request = api_request_text(method, api, path, body, token.as_deref());
    stream
        .write_all(request.as_bytes())
        .map_err(|source| ControlError::io(format!("write control request to {api}"), source))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|source| ControlError::io(format!("read control response from {api}"), source))?;
    let response = String::from_utf8_lossy(&response);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| ControlError::protocol("invalid API response"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ControlError::protocol("missing control response status line"))?;
    let status = response_status(status_line)?;
    if !(200..300).contains(&status) {
        return Err(ControlError::HttpStatus {
            status,
            body: body.to_string(),
        });
    }
    Ok(body.to_string())
}

fn response_status(status_line: &str) -> ControlResult<u16> {
    let mut parts = status_line.split_ascii_whitespace();
    let version = parts.next();
    let status = parts.next();
    if version != Some("HTTP/1.1") {
        return Err(ControlError::protocol(
            "invalid control response HTTP version",
        ));
    }
    status
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|status| (100..=599).contains(status))
        .ok_or_else(|| ControlError::protocol("invalid control response status"))
}

fn api_request_text(
    method: &str,
    api: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
) -> String {
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    format!(
        "{method} {path} HTTP/1.1\r\nHost: {api}\r\n{authorization}Connection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
}

#[cfg(test)]
#[path = "client/tests.rs"]
mod tests;
