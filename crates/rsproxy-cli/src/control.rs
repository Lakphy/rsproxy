use crate::app::{SharedState, unix_api_path, windows_pipe_path};
use crate::http;
#[cfg(unix)]
use std::fs;
use std::io::Write;
use std::net::TcpListener;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::thread;

#[cfg(unix)]
use std::os::unix::net::UnixListener;

mod auth;
mod query;
mod replay;
mod router;
mod routes;
mod values;

use router::handle;

pub(crate) enum ControlListener {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(UnixListener, PathBuf),
    #[cfg(windows)]
    WindowsPipe(crate::windows_pipe::NamedPipeListener),
}

impl ControlListener {
    pub(crate) fn endpoint(&self) -> std::io::Result<String> {
        match self {
            Self::Tcp(listener) => Ok(listener.local_addr()?.to_string()),
            #[cfg(unix)]
            Self::Unix(_, path) => Ok(format!("unix:{}", path.display())),
            #[cfg(windows)]
            Self::WindowsPipe(listener) => Ok(listener.endpoint()),
        }
    }
}

pub(crate) fn bind(addr: &str) -> std::io::Result<ControlListener> {
    if let Some(path) = unix_api_path(addr) {
        return bind_unix(path);
    }
    if let Some(path) = windows_pipe_path(addr) {
        return bind_windows_pipe(path);
    }
    TcpListener::bind(addr).map(ControlListener::Tcp)
}

pub(crate) fn serve(listener: ControlListener, state: SharedState) -> std::io::Result<()> {
    match listener {
        ControlListener::Tcp(listener) => serve_tcp(listener, state),
        #[cfg(unix)]
        ControlListener::Unix(listener, path) => serve_unix(listener, &path, state),
        #[cfg(windows)]
        ControlListener::WindowsPipe(listener) => serve_windows_pipe(listener, state),
    }
}

fn serve_tcp(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    let bound = listener.local_addr()?;
    tracing::info!(
        event = "control_listener_bound",
        transport = "tcp",
        address = %bound,
        "control listener bound"
    );
    for stream in listener.incoming() {
        let state = state.clone();
        match stream {
            Ok(stream) => {
                let peer = stream
                    .peer_addr()
                    .map(|address| address.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                thread::spawn(move || {
                    if let Err(error) = handle(stream, state) {
                        log_control_request_error(&error, "tcp", &peer);
                    }
                });
            }
            Err(error) => tracing::warn!(
                event = "control_accept_failed",
                transport = "tcp",
                address = %bound,
                error = %error,
                "control accept failed"
            ),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn bind_unix(path: &str) -> std::io::Result<ControlListener> {
    use std::os::unix::fs::PermissionsExt;

    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(ControlListener::Unix(listener, path.to_path_buf()))
}

#[cfg(unix)]
fn serve_unix(listener: UnixListener, path: &Path, state: SharedState) -> std::io::Result<()> {
    tracing::info!(
        event = "control_listener_bound",
        transport = "unix",
        address = %path.display(),
        "control listener bound"
    );
    for stream in listener.incoming() {
        let state = state.clone();
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(error) = handle(stream, state) {
                        log_control_request_error(&error, "unix", "local");
                    }
                });
            }
            Err(error) => tracing::warn!(
                event = "control_accept_failed",
                transport = "unix",
                address = %path.display(),
                error = %error,
                "control accept failed"
            ),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn bind_unix(_path: &str) -> std::io::Result<ControlListener> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unix control sockets are only supported on Unix",
    ))
}

#[cfg(windows)]
fn bind_windows_pipe(path: &str) -> std::io::Result<ControlListener> {
    crate::windows_pipe::NamedPipeListener::bind(path).map(ControlListener::WindowsPipe)
}

#[cfg(not(windows))]
fn bind_windows_pipe(path: &str) -> std::io::Result<ControlListener> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("Windows named pipes are not supported on this platform: {path}"),
    ))
}

#[cfg(windows)]
fn serve_windows_pipe(
    mut listener: crate::windows_pipe::NamedPipeListener,
    state: SharedState,
) -> std::io::Result<()> {
    tracing::info!(
        event = "control_listener_bound",
        transport = "windows-pipe",
        address = %listener.path(),
        "control listener bound"
    );
    loop {
        let stream = listener.accept()?;
        let state = state.clone();
        thread::spawn(move || {
            if let Err(error) = handle(stream, state) {
                log_control_request_error(&error, "windows-pipe", "local");
            }
        });
    }
}

fn log_control_request_error(error: &std::io::Error, transport: &str, peer: &str) {
    if expected_client_disconnect(error) {
        tracing::debug!(
            event = "control_client_disconnected",
            transport,
            peer,
            error = %error,
            "control client disconnected"
        );
    } else {
        tracing::warn!(
            event = "control_request_failed",
            transport,
            peer,
            error = %error,
            "control request failed"
        );
    }
}

fn expected_client_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::UnexpectedEof
    )
}

fn respond_json<W: Write + ?Sized>(stream: &mut W, status: u16, body: &str) -> std::io::Result<()> {
    http::write_response(
        stream,
        status,
        http::reason_phrase(status),
        &[("Content-Type".to_string(), "application/json".to_string())],
        body.as_bytes(),
    )
}

#[cfg(test)]
use auth::*;
#[cfg(test)]
use query::*;

#[cfg(test)]
#[path = "control/tests/mod.rs"]
mod tests;
