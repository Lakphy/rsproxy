use crate::{ControlError, ControlResult};
use rsproxy_engine::EngineHandle;
use rsproxy_trace::TraceStore;
#[cfg(unix)]
use std::fs;
use std::io::Write;
use std::net::TcpListener;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixListener;

mod auth;
mod http;
mod query;
mod router;
mod routes;
mod values;
#[cfg(windows)]
mod windows_pipe;
#[cfg(windows)]
pub(crate) use windows_pipe::NamedPipeStream;

use router::handle;

#[derive(Clone)]
pub struct ControlOptions {
    pub host: String,
    pub port: u16,
    pub api: String,
    pub api_token: Option<String>,
    pub storage: PathBuf,
    pub config_path: Option<PathBuf>,
    pub rules_watch: bool,
    pub rules_watch_debounce: Duration,
    pub max_header_size: usize,
    pub max_header_count: usize,
    pub max_body_size: usize,
}

impl std::fmt::Debug for ControlOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlOptions")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("api", &self.api)
            .field("api_token", &self.api_token.as_ref().map(|_| "<redacted>"))
            .field("storage", &self.storage)
            .field("config_path", &self.config_path)
            .field("rules_watch", &self.rules_watch)
            .field("rules_watch_debounce", &self.rules_watch_debounce)
            .field("max_header_size", &self.max_header_size)
            .field("max_header_count", &self.max_header_count)
            .field("max_body_size", &self.max_body_size)
            .finish()
    }
}

#[derive(Clone)]
pub struct ControlState {
    options: ControlOptions,
    engine: EngineHandle,
    trace: TraceStore,
}

impl ControlState {
    pub fn new(options: ControlOptions, engine: EngineHandle) -> Self {
        let trace = engine.trace_store();
        Self {
            options,
            engine,
            trace,
        }
    }
}

pub struct ControlListener(ControlListenerKind);

enum ControlListenerKind {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(UnixListener, PathBuf),
    #[cfg(windows)]
    WindowsPipe(windows_pipe::NamedPipeListener),
}

impl ControlListener {
    pub fn endpoint(&self) -> ControlResult<String> {
        match &self.0 {
            ControlListenerKind::Tcp(listener) => listener
                .local_addr()
                .map(|address| address.to_string())
                .map_err(|source| ControlError::io("read TCP control listener address", source)),
            #[cfg(unix)]
            ControlListenerKind::Unix(_, path) => Ok(format!("unix:{}", path.display())),
            #[cfg(windows)]
            ControlListenerKind::WindowsPipe(listener) => Ok(listener.endpoint()),
        }
    }
}

pub fn bind(addr: &str) -> ControlResult<ControlListener> {
    if let Some(path) = unix_api_path(addr) {
        return bind_unix(path);
    }
    if let Some(path) = windows_pipe_path(addr) {
        return bind_windows_pipe(path);
    }
    TcpListener::bind(addr)
        .map(|listener| ControlListener(ControlListenerKind::Tcp(listener)))
        .map_err(|source| ControlError::io(format!("bind TCP control listener {addr}"), source))
}

pub fn serve(listener: ControlListener, state: ControlState) -> ControlResult<()> {
    let result = match listener.0 {
        ControlListenerKind::Tcp(listener) => serve_tcp(listener, state),
        #[cfg(unix)]
        ControlListenerKind::Unix(listener, path) => serve_unix(listener, &path, state),
        #[cfg(windows)]
        ControlListenerKind::WindowsPipe(listener) => serve_windows_pipe(listener, state),
    };
    result.map_err(|source| ControlError::io("serve control listener", source))
}

fn serve_tcp(listener: TcpListener, state: ControlState) -> std::io::Result<()> {
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
fn bind_unix(path: &str) -> ControlResult<ControlListener> {
    use std::os::unix::fs::PermissionsExt;

    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            ControlError::io(
                format!("create Unix control socket directory {}", parent.display()),
                source,
            )
        })?;
    }
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(|source| {
        ControlError::io(
            format!("bind Unix control socket {}", path.display()),
            source,
        )
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        ControlError::io(
            format!("secure Unix control socket {}", path.display()),
            source,
        )
    })?;
    Ok(ControlListener(ControlListenerKind::Unix(
        listener,
        path.to_path_buf(),
    )))
}

#[cfg(unix)]
fn serve_unix(listener: UnixListener, path: &Path, state: ControlState) -> std::io::Result<()> {
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
fn bind_unix(_path: &str) -> ControlResult<ControlListener> {
    Err(ControlError::Unsupported(
        "unix control sockets are only supported on Unix".to_string(),
    ))
}

#[cfg(windows)]
fn bind_windows_pipe(path: &str) -> ControlResult<ControlListener> {
    windows_pipe::NamedPipeListener::bind(path)
        .map(|listener| ControlListener(ControlListenerKind::WindowsPipe(listener)))
        .map_err(|source| ControlError::io(format!("bind Windows control pipe {path}"), source))
}

#[cfg(not(windows))]
fn bind_windows_pipe(path: &str) -> ControlResult<ControlListener> {
    Err(ControlError::Unsupported(format!(
        "Windows named pipes are not supported on this platform: {path}"
    )))
}

#[cfg(windows)]
fn serve_windows_pipe(
    mut listener: windows_pipe::NamedPipeListener,
    state: ControlState,
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

pub fn unix_api_path(api: &str) -> Option<&str> {
    api.strip_prefix("unix://")
        .or_else(|| api.strip_prefix("unix:"))
        .filter(|path| !path.is_empty())
}

pub fn windows_pipe_path(api: &str) -> Option<&str> {
    api.strip_prefix("pipe://")
        .or_else(|| api.strip_prefix("pipe:"))
        .or_else(|| api.strip_prefix("npipe://"))
        .or_else(|| api.strip_prefix("npipe:"))
        .filter(|path| !path.is_empty())
}

#[cfg(test)]
use auth::*;
#[cfg(test)]
use query::*;

#[cfg(test)]
#[path = "server/tests/mod.rs"]
mod tests;
