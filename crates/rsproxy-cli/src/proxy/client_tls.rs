use super::*;

struct ClientTlsHandshakeStream<'a> {
    stream: &'a mut TcpStream,
    started: Instant,
    timeout: Duration,
    original_read_timeout: Option<Duration>,
    original_write_timeout: Option<Duration>,
    restored: bool,
}

impl<'a> ClientTlsHandshakeStream<'a> {
    fn new(stream: &'a mut TcpStream, timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stage=client_tls_handshake: timeout must be greater than zero",
            ));
        }
        Ok(Self {
            original_read_timeout: stream.read_timeout()?,
            original_write_timeout: stream.write_timeout()?,
            stream,
            started: Instant::now(),
            timeout,
            restored: false,
        })
    }

    fn prepare_io(&mut self) -> io::Result<()> {
        let remaining = self
            .timeout
            .checked_sub(self.started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "client TLS handshake deadline elapsed",
                )
            })?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.set_write_timeout(Some(remaining))
    }

    fn restore(&mut self) -> io::Result<()> {
        let read_result = self.stream.set_read_timeout(self.original_read_timeout);
        let write_result = self.stream.set_write_timeout(self.original_write_timeout);
        if read_result.is_ok() && write_result.is_ok() {
            self.restored = true;
        }
        read_result.and(write_result)
    }
}

impl Drop for ClientTlsHandshakeStream<'_> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.stream.set_read_timeout(self.original_read_timeout);
            let _ = self.stream.set_write_timeout(self.original_write_timeout);
        }
    }
}

impl Read for ClientTlsHandshakeStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.prepare_io()?;
        self.stream.read(buf)
    }
}

impl Write for ClientTlsHandshakeStream<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.prepare_io()?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.prepare_io()?;
        self.stream.flush()
    }
}

pub(super) fn complete_client_tls_handshake(
    conn: &mut ServerConnection,
    stream: &mut TcpStream,
    timeout: Duration,
) -> io::Result<u64> {
    let started = Instant::now();
    let mut stream = ClientTlsHandshakeStream::new(stream, timeout)?;
    let handshake = (|| {
        while conn.is_handshaking() {
            conn.complete_io(&mut stream)
                .map_err(|error| client_tls_handshake_io_error(error, timeout))?;
        }
        Ok(duration_millis(started.elapsed()))
    })();
    let restore = stream.restore();
    match (handshake, restore) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(stage_error("client_tls_io_restore", error)),
        (Ok(elapsed), Ok(())) => Ok(elapsed),
    }
}

pub(super) fn client_tls_handshake_io_error(error: io::Error, timeout: Duration) -> io::Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        return io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "stage=client_tls_handshake: timeout after {}ms",
                duration_millis(timeout)
            ),
        );
    }
    let kind = error.kind();
    io::Error::new(kind, format!("stage=client_tls: {error}"))
}

pub(super) fn is_client_tls_handshake_timeout(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut
        && error
            .to_string()
            .starts_with("stage=client_tls_handshake: timeout after ")
}
