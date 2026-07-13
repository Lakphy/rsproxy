use super::*;

pub(crate) enum UpstreamStream {
    Tcp(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, UpstreamStream>>),
}

pub(super) trait WsIo: Read + Write {
    fn set_ws_nonblocking(&mut self, nonblocking: bool) -> io::Result<()>;
    fn shutdown_ws(&mut self, how: Shutdown) -> io::Result<()>;
    fn set_request_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    fn try_clone_plain(&self) -> io::Result<Option<TcpStream>> {
        Ok(None)
    }
}

impl WsIo for TcpStream {
    fn set_ws_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.set_nonblocking(nonblocking)
    }

    fn shutdown_ws(&mut self, how: Shutdown) -> io::Result<()> {
        self.shutdown(how)
    }

    fn set_request_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }

    fn try_clone_plain(&self) -> io::Result<Option<TcpStream>> {
        self.try_clone().map(Some)
    }
}

impl WsIo for StreamOwned<ServerConnection, TcpStream> {
    fn set_ws_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.get_mut().set_nonblocking(nonblocking)
    }

    fn shutdown_ws(&mut self, how: Shutdown) -> io::Result<()> {
        self.conn.send_close_notify();
        self.get_mut().shutdown(how)
    }

    fn set_request_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.get_mut().set_read_timeout(timeout)
    }

    fn try_clone_plain(&self) -> io::Result<Option<TcpStream>> {
        Ok(None)
    }
}

impl UpstreamStream {
    pub(super) fn try_clone_tcp(&self) -> io::Result<Option<TcpStream>> {
        match self {
            UpstreamStream::Tcp(stream) => stream.try_clone().map(Some),
            UpstreamStream::Tls(_) => Ok(None),
        }
    }

    pub(super) fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        match self {
            UpstreamStream::Tcp(stream) => stream.set_nonblocking(nonblocking),
            UpstreamStream::Tls(stream) => stream.get_mut().set_nonblocking(nonblocking),
        }
    }

    pub(super) fn set_io_timeouts(
        &mut self,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> io::Result<()> {
        match self {
            UpstreamStream::Tcp(stream) => {
                stream.set_read_timeout(read_timeout)?;
                stream.set_write_timeout(write_timeout)
            }
            UpstreamStream::Tls(stream) => stream.sock.set_io_timeouts(read_timeout, write_timeout),
        }
    }

    pub(super) fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        match self {
            UpstreamStream::Tcp(stream) => stream.shutdown(how),
            UpstreamStream::Tls(stream) => {
                stream.conn.send_close_notify();
                stream.get_mut().shutdown(how)
            }
        }
    }

    pub(super) fn shutdown_socket(&mut self, how: Shutdown) -> io::Result<()> {
        match self {
            UpstreamStream::Tcp(stream) => stream.shutdown(how),
            UpstreamStream::Tls(stream) => stream.sock.shutdown_socket(how),
        }
    }

    pub(super) fn negotiated_h2(&self) -> bool {
        matches!(
            self,
            UpstreamStream::Tls(stream) if stream.conn.alpn_protocol() == Some(H2_ALPN)
        )
    }

    #[cfg(unix)]
    pub(super) fn raw_fd(&self) -> RawFd {
        match self {
            UpstreamStream::Tcp(stream) => stream.as_raw_fd(),
            UpstreamStream::Tls(stream) => stream.sock.raw_fd(),
        }
    }
}

impl Read for UpstreamStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            UpstreamStream::Tcp(stream) => stream.read(buf),
            UpstreamStream::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for UpstreamStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            UpstreamStream::Tcp(stream) => stream.write(buf),
            UpstreamStream::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            UpstreamStream::Tcp(stream) => stream.flush(),
            UpstreamStream::Tls(stream) => stream.flush(),
        }
    }
}

impl ReadyIo for UpstreamStream {
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        UpstreamStream::set_nonblocking(self, nonblocking)
    }

    fn begin_shutdown(&mut self) {
        if let UpstreamStream::Tls(stream) = self {
            stream.conn.send_close_notify();
        }
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.shutdown_socket(Shutdown::Write)
    }

    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        UpstreamStream::raw_fd(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UpstreamProtocol {
    Http1,
    Http1Pooled { reused_connection: bool },
    Http2 { reused_connection: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClientPersistence {
    Close,
    KeepAlive,
}

impl ClientPersistence {
    pub(super) fn keep_alive(self) -> bool {
        self == Self::KeepAlive
    }
}

pub(super) fn requested_client_connection(request: &RawRequest) -> ClientPersistence {
    let explicitly_closes = header_contains_token(&request.headers, "connection", "close")
        || header_contains_token(&request.headers, "proxy-connection", "close");
    if explicitly_closes {
        return ClientPersistence::Close;
    }
    if request.version.eq_ignore_ascii_case("HTTP/1.1") {
        return ClientPersistence::KeepAlive;
    }
    if request.version.eq_ignore_ascii_case("HTTP/1.0")
        && (header_contains_token(&request.headers, "connection", "keep-alive")
            || header_contains_token(&request.headers, "proxy-connection", "keep-alive"))
    {
        return ClientPersistence::KeepAlive;
    }
    ClientPersistence::Close
}

pub(super) fn client_response_version(request_version: &str) -> &str {
    if request_version.eq_ignore_ascii_case("HTTP/1.0") {
        "HTTP/1.0"
    } else {
        "HTTP/1.1"
    }
}

pub(super) fn header_contains_token(
    headers: &[(String, String)],
    name: &str,
    wanted: &str,
) -> bool {
    headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .any(|token| token.eq_ignore_ascii_case(wanted))
}

pub(super) struct ForwardResult {
    pub(super) status: u16,
    pub(super) upstream: String,
    pub(super) request_bytes: u64,
    pub(super) request_body_head: Option<Vec<u8>>,
    pub(super) request_trailers: Option<Vec<(String, String)>>,
    pub(super) response_bytes: u64,
    pub(super) res_headers: Vec<(String, String)>,
    pub(super) res_trailers: Vec<(String, String)>,
    pub(super) body_head: Vec<u8>,
    pub(super) frames: Vec<FrameRecord>,
    pub(super) kind: Option<SessionKind>,
    pub(super) response_matched_rules: Vec<MatchedRule>,
    pub(super) response_actions: Vec<ResolvedAction>,
    pub(super) protocol: UpstreamProtocol,
    pub(super) client_connection: ClientPersistence,
    pub(super) pool_wait_ms: u64,
    pub(super) request_send_ms: Option<u64>,
    pub(super) response_receive_ms: Option<u64>,
    pub(super) flags: Vec<String>,
    pub(super) error: Option<String>,
}

#[derive(Default)]
pub(super) struct NetworkTimings {
    pub(super) dns_ms: u64,
    pub(super) connect_ms: u64,
    pub(super) ttfb_ms: u64,
    pub(super) request_send_ms: Option<u64>,
    pub(super) response_receive_ms: Option<u64>,
}
