use super::*;

#[derive(Clone)]
pub(super) struct TunnelTrace {
    store: rsproxy_trace::TraceStore,
    id: u64,
}

impl TunnelTrace {
    pub(super) fn new(store: rsproxy_trace::TraceStore, id: u64) -> Option<Self> {
        (id != 0).then_some(Self { store, id })
    }

    fn observe(&self, direction: rsproxy_trace::BodyDirection, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.store.emit(rsproxy_trace::TraceEvent::BodyChunk {
            id: self.id,
            direction,
            data: bytes::Bytes::new(),
            observed_bytes: bytes as u64,
        });
    }
}

pub(super) fn tunnel_copy(
    client: TcpStream,
    upstream: UpstreamStream,
    trace: Option<TunnelTrace>,
) -> (u64, u64) {
    match upstream {
        UpstreamStream::Tcp(upstream) => tunnel_copy_tcp(client, upstream, trace),
        UpstreamStream::Tls(upstream) => tunnel_copy_tls(client, *upstream, trace),
    }
}

pub(super) fn tunnel_copy_tcp(
    client: TcpStream,
    upstream: TcpStream,
    trace: Option<TunnelTrace>,
) -> (u64, u64) {
    let mut client_r = match client.try_clone() {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };
    let mut client_w = client;
    let mut upstream_r = match upstream.try_clone() {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };
    let mut upstream_w = upstream;

    let request_trace = trace.clone();
    let up = thread::spawn(move || {
        let n = copy_tunnel_direction(
            &mut client_r,
            &mut upstream_w,
            request_trace.as_ref(),
            rsproxy_trace::BodyDirection::Request,
        );
        let _ = upstream_w.shutdown(Shutdown::Write);
        n
    });
    let down = thread::spawn(move || {
        let n = copy_tunnel_direction(
            &mut upstream_r,
            &mut client_w,
            trace.as_ref(),
            rsproxy_trace::BodyDirection::Response,
        );
        let _ = client_w.shutdown(Shutdown::Write);
        n
    });
    (up.join().unwrap_or(0), down.join().unwrap_or(0))
}

fn copy_tunnel_direction(
    reader: &mut TcpStream,
    writer: &mut TcpStream,
    trace: Option<&TunnelTrace>,
    direction: rsproxy_trace::BodyDirection,
) -> u64 {
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let size = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => size,
            Err(_) => break,
        };
        if writer.write_all(&buffer[..size]).is_err() {
            break;
        }
        total = total.saturating_add(size as u64);
        if let Some(trace) = trace {
            trace.observe(direction, size);
        }
    }
    total
}

pub(super) fn tunnel_copy_tls(
    client: TcpStream,
    upstream: StreamOwned<ClientConnection, UpstreamStream>,
    trace: Option<TunnelTrace>,
) -> (u64, u64) {
    let mut request_bytes = 0u64;
    let mut response_bytes = 0u64;
    let _ = tunnel_copy_tls_inner(
        client,
        upstream,
        trace.as_ref(),
        &mut request_bytes,
        &mut response_bytes,
    );
    (request_bytes, response_bytes)
}

fn tunnel_copy_tls_inner(
    mut client: TcpStream,
    upstream: StreamOwned<ClientConnection, UpstreamStream>,
    trace: Option<&TunnelTrace>,
    request_bytes: &mut u64,
    response_bytes: &mut u64,
) -> io::Result<()> {
    let (mut conn, mut proxy) = upstream.into_parts();
    client.set_nonblocking(true)?;
    proxy.set_nonblocking(true)?;

    let mut client_closed = false;
    let mut proxy_closed = false;
    let mut pending_to_tls = Vec::new();
    let mut pending_to_client = Vec::new();
    let mut client_buf = [0u8; 8192];
    let mut tls_buf = [0u8; 8192];

    loop {
        let mut progressed = false;

        if flush_tls_plaintext(&mut conn, &mut pending_to_tls)? {
            progressed = true;
        }
        if flush_tls_records(&mut conn, &mut proxy)? {
            progressed = true;
        }

        if !client_closed && pending_to_tls.len() < 1024 * 1024 {
            loop {
                match client.read(&mut client_buf) {
                    Ok(0) => {
                        client_closed = true;
                        conn.send_close_notify();
                        progressed = true;
                        break;
                    }
                    Ok(n) => {
                        *request_bytes = request_bytes.saturating_add(n as u64);
                        if let Some(trace) = trace {
                            trace.observe(rsproxy_trace::BodyDirection::Request, n);
                        }
                        pending_to_tls.extend_from_slice(&client_buf[..n]);
                        progressed = true;
                        if pending_to_tls.len() >= 1024 * 1024 {
                            break;
                        }
                    }
                    Err(err) if would_block(&err) => break,
                    Err(err) if tunnel_end_error(&err) => {
                        client_closed = true;
                        conn.send_close_notify();
                        progressed = true;
                        break;
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        if flush_tls_plaintext(&mut conn, &mut pending_to_tls)? {
            progressed = true;
        }
        if flush_tls_records(&mut conn, &mut proxy)? {
            progressed = true;
        }

        if !proxy_closed && conn.wants_read() {
            loop {
                match conn.read_tls(&mut proxy) {
                    Ok(0) => {
                        proxy_closed = true;
                        progressed = true;
                        break;
                    }
                    Ok(_) => {
                        conn.process_new_packets()
                            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                        loop {
                            match conn.reader().read(&mut tls_buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    pending_to_client.extend_from_slice(&tls_buf[..n]);
                                }
                                Err(err) if would_block(&err) => break,
                                Err(err) => return Err(err),
                            }
                        }
                        progressed = true;
                    }
                    Err(err) if would_block(&err) => break,
                    Err(err) if tls_close_notify_missing(&err) || tunnel_end_error(&err) => {
                        proxy_closed = true;
                        progressed = true;
                        break;
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        let written = flush_pending_to_stream(&mut client, &mut pending_to_client)?;
        if written > 0 {
            *response_bytes = response_bytes.saturating_add(written as u64);
            if let Some(trace) = trace {
                trace.observe(rsproxy_trace::BodyDirection::Response, written);
            }
            progressed = true;
        }
        if flush_tls_records(&mut conn, &mut proxy)? {
            progressed = true;
        }

        if proxy_closed && pending_to_client.is_empty() {
            let _ = client.shutdown(Shutdown::Write);
            break;
        }
        if client_closed
            && proxy_closed
            && pending_to_tls.is_empty()
            && pending_to_client.is_empty()
        {
            break;
        }

        if !progressed {
            thread::sleep(Duration::from_millis(1));
        }
    }

    let _ = proxy.shutdown(Shutdown::Both);
    let _ = client.shutdown(Shutdown::Both);
    Ok(())
}

pub(super) fn flush_tls_plaintext(
    conn: &mut ClientConnection,
    pending: &mut Vec<u8>,
) -> io::Result<bool> {
    let mut progressed = false;
    while !pending.is_empty() {
        match conn.writer().write(pending) {
            Ok(0) => break,
            Ok(n) => {
                pending.drain(..n);
                progressed = true;
            }
            Err(err) if would_block(&err) => break,
            Err(err) => return Err(err),
        }
    }
    Ok(progressed)
}

pub(super) fn flush_tls_records<S: Write>(
    conn: &mut ClientConnection,
    proxy: &mut S,
) -> io::Result<bool> {
    let mut progressed = false;
    while conn.wants_write() {
        match conn.write_tls(proxy) {
            Ok(0) => break,
            Ok(_) => progressed = true,
            Err(err) if would_block(&err) => break,
            Err(err) if tunnel_end_error(&err) => break,
            Err(err) => return Err(err),
        }
    }
    Ok(progressed)
}

pub(super) fn flush_pending_to_stream(
    stream: &mut TcpStream,
    pending: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut written = 0usize;
    while !pending.is_empty() {
        match stream.write(pending) {
            Ok(0) => break,
            Ok(n) => {
                pending.drain(..n);
                written += n;
            }
            Err(err) if would_block(&err) => break,
            Err(err) if tunnel_end_error(&err) => break,
            Err(err) => return Err(err),
        }
    }
    Ok(written)
}

pub(super) fn would_block(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

pub(super) fn tunnel_end_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    )
}

#[cfg(test)]
#[path = "tunnel/tests.rs"]
mod tests;
