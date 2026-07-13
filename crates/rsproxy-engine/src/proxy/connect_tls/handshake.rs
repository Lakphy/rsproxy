use super::*;

pub(in crate::proxy) struct TlsWrapInput<'a> {
    pub tls_host: &'a str,
    pub client_identity: Option<TlsClientIdentity>,
    pub tls_policy: Option<&'a TlsOp>,
    pub allow_h2: bool,
    pub state: &'a SharedState,
    pub deadline: RequestDeadline,
}

pub(in crate::proxy) fn tls_wrap_upstream_stream(
    upstream: UpstreamStream,
    input: TlsWrapInput<'_>,
    tls_records: &mut Vec<TlsRecord>,
) -> io::Result<UpstreamStream> {
    let TlsWrapInput {
        tls_host,
        client_identity,
        tls_policy,
        allow_h2,
        state,
        deadline,
    } = input;
    let started_ms = rsproxy_trace::now_millis();
    let handshake_budget = deadline.budget(state.config.upstream_tls_handshake_timeout)?;
    let handshake_timeout = handshake_budget.timeout();
    match tls_wrap_stream(
        upstream,
        tls_host,
        client_identity,
        tls_policy,
        allow_h2,
        state,
        handshake_timeout,
    ) {
        Ok((mut stream, record)) => {
            stream
                .sock
                .set_io_timeouts(Some(UPSTREAM_READ_TIMEOUT), Some(UPSTREAM_WRITE_TIMEOUT))?;
            tls_records.push(record);
            Ok(UpstreamStream::Tls(Box::new(stream)))
        }
        Err(err) => {
            let err = handshake_budget.map_timeout(err);
            tls_records.push(failed_tls_record(
                "upstream_tls",
                tls_host,
                started_ms,
                &err,
            ));
            Err(err)
        }
    }
}

fn tls_wrap_stream(
    stream: UpstreamStream,
    tls_host: &str,
    client_identity: Option<TlsClientIdentity>,
    tls_policy: Option<&TlsOp>,
    allow_h2: bool,
    state: &SharedState,
    handshake_timeout: Duration,
) -> io::Result<(StreamOwned<ClientConnection, UpstreamStream>, TlsRecord)> {
    let config = mitm_client_config(state, client_identity, tls_policy, allow_h2)
        .map_err(|err| stage_error("tls_config", err))?;
    let server_name = ServerName::try_from(tls_host.to_string())
        .map_err(|_| stage_error("tls", "invalid TLS server name"))?;
    let mut conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|err| stage_error("tls", err))?;
    let handshake_started = rsproxy_trace::now_millis();
    let mut handshake_stream = TlsHandshakeStream::new(stream, handshake_timeout);
    while conn.is_handshaking() {
        conn.complete_io(&mut handshake_stream)
            .map_err(|err| tls_handshake_io_error(err, handshake_timeout))?;
    }
    let record = client_tls_record(
        "upstream_tls",
        tls_host,
        rsproxy_trace::now_millis().saturating_sub(handshake_started),
        &conn,
    );
    Ok((
        StreamOwned::new(conn, handshake_stream.into_inner()),
        record,
    ))
}

struct TlsHandshakeStream {
    stream: UpstreamStream,
    started: Instant,
    timeout: Duration,
}

impl TlsHandshakeStream {
    fn new(stream: UpstreamStream, timeout: Duration) -> Self {
        Self {
            stream,
            started: Instant::now(),
            timeout,
        }
    }

    fn prepare_io(&mut self) -> io::Result<()> {
        let remaining = self
            .timeout
            .checked_sub(self.started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "TLS handshake deadline elapsed")
            })?;
        self.stream
            .set_io_timeouts(Some(remaining), Some(remaining))
    }

    fn into_inner(self) -> UpstreamStream {
        self.stream
    }
}

impl Read for TlsHandshakeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.prepare_io()?;
        self.stream.read(buf)
    }
}

impl Write for TlsHandshakeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.prepare_io()?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.prepare_io()?;
        self.stream.flush()
    }
}

pub(in crate::proxy) fn tls_handshake_io_error(error: io::Error, timeout: Duration) -> io::Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        return io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "stage=tls_handshake: timeout after {}ms",
                timeout.as_millis()
            ),
        );
    }
    io::Error::new(error.kind(), format!("stage=tls: {error}"))
}
