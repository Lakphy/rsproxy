use super::*;
use std::io::{BufRead, BufReader};

pub(super) fn set_api_token(token: Option<String>) {
    *api_token_state().write().expect("API token state poisoned") = token;
}

fn api_token_state() -> &'static std::sync::RwLock<Option<String>> {
    static TOKEN: std::sync::OnceLock<std::sync::RwLock<Option<String>>> =
        std::sync::OnceLock::new();
    TOKEN.get_or_init(|| std::sync::RwLock::new(None))
}

fn configured_api_token() -> Option<String> {
    api_token_state()
        .read()
        .expect("API token state poisoned")
        .clone()
}

pub(crate) fn api_request(
    method: &str,
    api: &str,
    path: &str,
    body: &str,
) -> Result<String, String> {
    use std::net::TcpStream;
    use std::time::Duration;

    if let Some(socket_path) = unix_api_path(api) {
        return api_request_unix(method, api, socket_path, path, body);
    }
    if let Some(pipe_path) = crate::app::windows_pipe_path(api) {
        return api_request_windows_pipe(method, api, pipe_path, path, body);
    }
    let mut stream = TcpStream::connect(api).map_err(|e| format!("connect {api}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    api_request_stream(&mut stream, method, api, path, body)
}

pub(crate) fn api_stream_lines(
    api: &str,
    path: &str,
    mut on_line: impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    use std::net::TcpStream;

    if let Some(socket_path) = unix_api_path(api) {
        return api_stream_lines_unix(api, socket_path, path, &mut on_line);
    }
    if let Some(pipe_path) = crate::app::windows_pipe_path(api) {
        return api_stream_lines_windows_pipe(api, pipe_path, path, &mut on_line);
    }
    let mut stream = TcpStream::connect(api).map_err(|error| format!("connect {api}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(65)))
        .map_err(|error| error.to_string())?;
    api_stream_lines_from(&mut stream, api, path, &mut on_line)
}

#[cfg(unix)]
fn api_stream_lines_unix(
    api: &str,
    socket_path: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("connect unix socket {socket_path}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(65)))
        .map_err(|error| error.to_string())?;
    api_stream_lines_from(&mut stream, api, path, on_line)
}

#[cfg(windows)]
fn api_stream_lines_windows_pipe(
    api: &str,
    pipe_path: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    let mut stream = crate::windows_pipe::NamedPipeStream::connect(pipe_path)
        .map_err(|error| format!("connect Windows named pipe {pipe_path}: {error}"))?;
    api_stream_lines_from(&mut stream, api, path, on_line)
}

#[cfg(not(windows))]
fn api_stream_lines_windows_pipe(
    _api: &str,
    pipe_path: &str,
    _path: &str,
    _on_line: &mut impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    Err(format!(
        "Windows named pipes are not supported on this platform: {pipe_path}"
    ))
}

#[cfg(not(unix))]
fn api_stream_lines_unix(
    _api: &str,
    socket_path: &str,
    _path: &str,
    _on_line: &mut impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    Err(format!(
        "unix control sockets are not supported on this platform: {socket_path}"
    ))
}

pub(super) fn api_stream_lines_from<S: Read + Write>(
    stream: &mut S,
    api: &str,
    path: &str,
    on_line: &mut impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    let request = api_request_text("GET", api, path, "", configured_api_token().as_deref());
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err("control API closed before the response head".to_string());
    }
    let success = line.starts_with("HTTP/1.1 2");
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("control API closed during the response head".to_string());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    if !success {
        let mut body = String::new();
        reader
            .read_to_string(&mut body)
            .map_err(|error| error.to_string())?;
        return Err(body);
    }

    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("trace follow stream closed".to_string());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if !line.is_empty() && !on_line(line)? {
            return Ok(());
        }
    }
}

#[cfg(unix)]
pub(super) fn api_request_unix(
    method: &str,
    api: &str,
    socket_path: &str,
    path: &str,
    body: &str,
) -> Result<String, String> {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("connect unix socket {socket_path}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    api_request_stream(&mut stream, method, api, path, body)
}

#[cfg(windows)]
fn api_request_windows_pipe(
    method: &str,
    api: &str,
    pipe_path: &str,
    path: &str,
    body: &str,
) -> Result<String, String> {
    let mut stream = crate::windows_pipe::NamedPipeStream::connect(pipe_path)
        .map_err(|error| format!("connect Windows named pipe {pipe_path}: {error}"))?;
    api_request_stream(&mut stream, method, api, path, body)
}

#[cfg(not(windows))]
fn api_request_windows_pipe(
    _method: &str,
    _api: &str,
    pipe_path: &str,
    _path: &str,
    _body: &str,
) -> Result<String, String> {
    Err(format!(
        "Windows named pipes are not supported on this platform: {pipe_path}"
    ))
}

#[cfg(not(unix))]
pub(super) fn api_request_unix(
    _method: &str,
    _api: &str,
    socket_path: &str,
    _path: &str,
    _body: &str,
) -> Result<String, String> {
    Err(format!(
        "unix control sockets are not supported on this platform: {socket_path}"
    ))
}

pub(super) fn api_request_stream<S: Read + Write>(
    stream: &mut S,
    method: &str,
    api: &str,
    path: &str,
    body: &str,
) -> Result<String, String> {
    let token = configured_api_token();
    let req = api_request_text(method, api, path, body, token.as_deref());
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    let response = String::from_utf8_lossy(&response);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid API response".to_string())?;
    if !head.starts_with("HTTP/1.1 2") {
        return Err(body.to_string());
    }
    Ok(body.to_string())
}

pub(super) fn api_request_text(
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
