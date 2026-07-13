use super::*;

struct TtfbReadStream<'a> {
    stream: &'a mut UpstreamStream,
    started: Instant,
    timeout: Duration,
    deadline: RequestDeadline,
    first_byte_ms: Option<u64>,
}

impl<'a> TtfbReadStream<'a> {
    fn new(
        stream: &'a mut UpstreamStream,
        timeout: Duration,
        deadline: RequestDeadline,
    ) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stage=ttfb: timeout must be greater than zero",
            ));
        }
        Ok(Self {
            stream,
            started: Instant::now(),
            timeout,
            deadline,
            first_byte_ms: None,
        })
    }

    fn prepare_read(&mut self) -> io::Result<Option<TimeoutBudget>> {
        let (read_timeout, budget) = if self.first_byte_ms.is_some() {
            let budget = self.deadline.budget(UPSTREAM_READ_TIMEOUT)?;
            (budget.timeout(), Some(budget))
        } else {
            (
                self.timeout
                    .checked_sub(self.started.elapsed())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::TimedOut, "TTFB deadline elapsed")
                    })?,
                None,
            )
        };
        self.stream
            .set_io_timeouts(Some(read_timeout), Some(UPSTREAM_WRITE_TIMEOUT))?;
        Ok(budget)
    }
}

impl Read for TtfbReadStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let budget = self.prepare_read()?;
        let read = self.stream.read(buf).map_err(|error| match budget {
            Some(budget) => budget.map_timeout(error),
            None => error,
        });
        if matches!(read, Ok(size) if size > 0) && self.first_byte_ms.is_none() {
            self.first_byte_ms = Some(duration_millis(self.started.elapsed()));
        }
        read
    }
}

pub(in crate::proxy) fn read_response_head_with_ttfb(
    stream: &mut UpstreamStream,
    max_header_size: usize,
    max_header_count: usize,
    timeout: Duration,
    deadline: RequestDeadline,
    network_timings: &mut NetworkTimings,
) -> io::Result<http::RawResponseHead> {
    let budget = deadline.budget(timeout)?;
    let (result, first_byte_ms) = {
        let mut reader = TtfbReadStream::new(stream, budget.timeout(), deadline)?;
        let result = http::read_response_head(&mut reader, max_header_size, max_header_count);
        (result, reader.first_byte_ms)
    };
    stream.set_io_timeouts(Some(UPSTREAM_READ_TIMEOUT), Some(UPSTREAM_WRITE_TIMEOUT))?;
    if let Some(ttfb_ms) = first_byte_ms {
        network_timings.ttfb_ms = network_timings.ttfb_ms.saturating_add(ttfb_ms);
    }
    match result {
        Err(error)
            if first_byte_ms.is_none()
                && matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
        {
            Err(budget.timeout_error(ttfb_timeout_error))
        }
        Err(error) => Err(stage_io_error("response_head", error)),
        Ok(head) => Ok(head),
    }
}

fn ttfb_timeout_error(timeout: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("stage=ttfb: timeout after {}ms", duration_millis(timeout)),
    )
}

pub(in crate::proxy) fn connect_tcp_with_timeouts(
    addr: &str,
    state: &SharedState,
    network_timings: &mut NetworkTimings,
    deadline: RequestDeadline,
) -> io::Result<TcpStream> {
    let dns_started = Instant::now();
    let dns_budget = deadline.budget(state.config.dns_timeout)?;
    let addresses = state
        .dns_resolver
        .resolve_socket_addrs_with_timeout(addr, dns_budget.timeout())
        .map_err(|error| dns_budget.map_timeout(error));
    network_timings.dns_ms = network_timings
        .dns_ms
        .saturating_add(duration_millis(dns_started.elapsed()));
    let addresses = addresses?;
    if addresses.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("stage=dns: no addresses resolved for {addr}"),
        ));
    }

    let connect_budget = deadline.budget(state.config.tcp_connect_timeout)?;
    let timeout = connect_budget.timeout();
    let started = Instant::now();
    let result = (|| {
        let mut last_error = None;
        for address in addresses {
            let Some(remaining) = timeout
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
            else {
                return Err(tcp_connect_timeout_error(timeout, addr));
            };
            match TcpStream::connect_timeout(&address, remaining) {
                Ok(tcp) => return Ok(tcp),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Err(tcp_connect_timeout_error(timeout, addr));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(staged_io_error(
            "connect",
            last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "no address was connectable",
                )
            }),
        ))
    })();
    network_timings.connect_ms = network_timings
        .connect_ms
        .saturating_add(duration_millis(started.elapsed()));
    let tcp = result.map_err(|error| connect_budget.map_timeout(error))?;
    tcp.set_read_timeout(Some(UPSTREAM_READ_TIMEOUT))?;
    tcp.set_write_timeout(Some(UPSTREAM_WRITE_TIMEOUT))?;
    Ok(tcp)
}

pub(in crate::proxy) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

pub(in crate::proxy) fn tcp_connect_timeout_error(timeout: Duration, addr: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "stage=connect: timeout after {}ms connecting to {addr}",
            timeout.as_millis()
        ),
    )
}

pub(in crate::proxy) fn staged_io_error(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("stage={stage}: {error}"))
}
