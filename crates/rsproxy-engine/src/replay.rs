use crate::{EngineError, EngineResult, ReplayResponse, SharedState};
use rsproxy_net::RequestDeadline;
use rsproxy_rules::UrlParts;
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use std::io;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod body;

use body::read_response_body;

pub(crate) fn replay_session(
    session: &rsproxy_trace::Session,
    state: &SharedState,
) -> EngineResult<ReplayResponse> {
    let deadline = RequestDeadline::new(state.config.request_total_timeout)
        .map_err(|source| replay_io("start replay deadline", source))?;
    let url = UrlParts::parse(&session.url)?;
    if !matches!(url.scheme.as_str(), "http" | "https") {
        return Err(EngineError::Unsupported(
            "replay supports http and https URLs only".to_string(),
        ));
    }
    let port = url
        .effective_port()
        .unwrap_or(if url.scheme == "https" { 443 } else { 80 });
    let address = format!("{}:{port}", url.host);
    let tcp = connect_origin(&address, state, deadline)?;
    let mut upstream = if url.scheme == "https" {
        ReplayStream::Tls(Box::new(connect_tls(tcp, &url.host, state, deadline)?))
    } else {
        ReplayStream::Tcp(tcp)
    };
    upstream
        .set_write_timeout(
            deadline
                .remaining()
                .map_err(|source| replay_io("prepare replay request write", source))?,
        )
        .map_err(|source| replay_io("configure replay write timeout", source))?;
    let mut headers = session.req_headers.clone();
    rsproxy_net::remove_header(&mut headers, "proxy-connection");
    rsproxy_net::remove_header(&mut headers, "connection");
    rsproxy_net::remove_header(&mut headers, "content-length");
    rsproxy_net::set_header(&mut headers, "Host", host_header(&url));
    rsproxy_net::set_header(&mut headers, "Connection", "close".to_string());
    if !session.req_body_head.is_empty() {
        rsproxy_net::set_header(
            &mut headers,
            "Content-Length",
            session.req_body_head.len().to_string(),
        );
    }

    write!(
        upstream,
        "{} {} HTTP/1.1\r\n",
        session.method,
        url.origin_form()
    )
    .map_err(|source| replay_io("write replay request line", source))?;
    for (name, value) in &headers {
        write!(upstream, "{name}: {value}\r\n")
            .map_err(|source| replay_io("write replay request header", source))?;
    }
    write!(upstream, "\r\n").map_err(|source| replay_io("finish replay request head", source))?;
    if !session.req_body_head.is_empty() {
        upstream
            .write_all(&session.req_body_head)
            .map_err(|source| replay_io("write replay request body", source))?;
    }

    let mut reader = ReplayReader::new(&mut upstream, deadline, state.config.upstream_ttfb_timeout);
    let head = rsproxy_net::read_response_head(
        &mut reader,
        state.config.max_header_size,
        state.config.max_header_count,
    )
    .map_err(|source| replay_io("read replay response head", source))?;
    let (response_bytes, body_head) = read_response_body(
        &mut reader,
        &session.method,
        head.status,
        &head.headers,
        state.config.max_header_size,
        state.config.max_header_count,
    )
    .map_err(|source| replay_io("read replay response body", source))?;
    Ok(ReplayResponse {
        status: head.status,
        response_bytes,
        headers: head.headers,
        body_head,
    })
}

fn connect_tls(
    mut stream: TcpStream,
    host: &str,
    state: &SharedState,
    deadline: RequestDeadline,
) -> EngineResult<StreamOwned<ClientConnection, TcpStream>> {
    let budget = deadline
        .budget(state.config.upstream_tls_handshake_timeout)
        .map_err(|source| replay_io("prepare replay TLS handshake", source))?;
    stream
        .set_read_timeout(Some(budget.timeout()))
        .map_err(|source| replay_io("configure replay TLS read timeout", source))?;
    stream
        .set_write_timeout(Some(budget.timeout()))
        .map_err(|source| replay_io("configure replay TLS write timeout", source))?;
    let config = crate::proxy::tls::replay_client_config(state)
        .map_err(|source| replay_io("configure replay TLS client", source))?;
    let server_name = ServerName::try_from(host.to_string()).map_err(|_| {
        EngineError::InvalidInput(format!("invalid replay TLS server name `{host}`"))
    })?;
    let mut connection = ClientConnection::new(Arc::new(config), server_name).map_err(|error| {
        EngineError::InvalidInput(format!("initialize replay TLS client: {error}"))
    })?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut stream)
            .map_err(|source| replay_io("complete replay TLS handshake", source))?;
    }
    Ok(StreamOwned::new(connection, stream))
}

fn connect_origin(
    target: &str,
    state: &SharedState,
    deadline: RequestDeadline,
) -> EngineResult<TcpStream> {
    let dns_budget = deadline
        .budget(state.config.dns_timeout)
        .map_err(|source| replay_io("prepare replay DNS lookup", source))?;
    let addresses = state
        .dns_resolver
        .resolve_socket_addrs_with_timeout(target, dns_budget.timeout())
        .map_err(|source| replay_io("resolve replay origin", dns_budget.map_timeout(source)))?;
    let connect_budget = deadline
        .budget(state.config.tcp_connect_timeout)
        .map_err(|source| replay_io("prepare replay origin connection", source))?;
    connect_first(&addresses, connect_budget.timeout()).map_err(|source| EngineError::Io {
        context: format!("connect replay origin {target}"),
        source: connect_budget.map_timeout(source),
    })
}

fn connect_first(addresses: &[SocketAddr], timeout: Duration) -> io::Result<TcpStream> {
    let started = Instant::now();
    let mut last_error = None;
    for address in addresses {
        let Some(remaining) = timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
        else {
            break;
        };
        match TcpStream::connect_timeout(address, remaining) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "replay connection timed out after {}ms",
                timeout.as_millis()
            ),
        )
    }))
}

enum ReplayStream {
    Tcp(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl ReplayStream {
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(Some(timeout)),
            Self::Tls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        }
    }

    fn set_write_timeout(&self, timeout: Duration) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_write_timeout(Some(timeout)),
            Self::Tls(stream) => stream.get_ref().set_write_timeout(Some(timeout)),
        }
    }
}

impl Read for ReplayStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            Self::Tls(stream) => stream.read(buffer),
        }
    }
}

impl Write for ReplayStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            Self::Tls(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

struct ReplayReader<'a> {
    stream: &'a mut ReplayStream,
    deadline: RequestDeadline,
    ttfb_started: Instant,
    ttfb_timeout: Duration,
    received_first_byte: bool,
}

impl<'a> ReplayReader<'a> {
    fn new(
        stream: &'a mut ReplayStream,
        deadline: RequestDeadline,
        ttfb_timeout: Duration,
    ) -> Self {
        Self {
            stream,
            deadline,
            ttfb_started: Instant::now(),
            ttfb_timeout,
            received_first_byte: false,
        }
    }

    fn read_timeout(&self) -> io::Result<Duration> {
        let remaining = self.deadline.remaining()?;
        if self.received_first_byte {
            return Ok(remaining);
        }
        let ttfb_remaining = self
            .ttfb_timeout
            .checked_sub(self.ttfb_started.elapsed())
            .filter(|timeout| !timeout.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "replay response timed out after {}ms",
                        self.ttfb_timeout.as_millis()
                    ),
                )
            })?;
        Ok(remaining.min(ttfb_remaining))
    }
}

impl Read for ReplayReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(self.read_timeout()?)?;
        let size = self.stream.read(buffer)?;
        self.received_first_byte |= size != 0;
        Ok(size)
    }
}

fn replay_io(context: &str, source: io::Error) -> EngineError {
    EngineError::Io {
        context: context.to_string(),
        source,
    }
}

fn host_header(url: &UrlParts) -> String {
    match (url.port, url.scheme.as_str()) {
        (Some(80), "http" | "ws") | (Some(443), "https" | "wss") | (None, _) => url.host.clone(),
        (Some(port), _) => format!("{}:{port}", url.host),
    }
}

#[cfg(test)]
mod tests;
