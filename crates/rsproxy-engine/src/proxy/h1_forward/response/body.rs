use super::*;

const RELAY_BUFFER_SIZE: usize = 16 * 1024;

pub(super) struct BodySummary {
    pub(super) bytes: u64,
    pub(super) body_head: Vec<u8>,
    pub(super) limit: usize,
    pub(super) streamed: bool,
}

impl BodySummary {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            bytes: 0,
            body_head: Vec::with_capacity(limit.min(SMALL_BODY_LIMIT)),
            limit,
            streamed: true,
        }
    }

    pub(super) fn empty() -> Self {
        Self {
            bytes: 0,
            body_head: Vec::new(),
            limit: 0,
            streamed: false,
        }
    }

    pub(super) fn completed(bytes: u64, body_head: Vec<u8>) -> Self {
        Self {
            bytes,
            limit: body_head.len(),
            body_head,
            streamed: true,
        }
    }

    fn observe(&mut self, data: &[u8]) {
        self.bytes = self.bytes.saturating_add(data.len() as u64);
        let remaining = self.limit.saturating_sub(self.body_head.len());
        self.body_head.extend(data.iter().copied().take(remaining));
    }
}

pub(super) fn body_trace<'a>(
    state: &'a SharedState,
    trace_id: u64,
    limit: usize,
) -> Option<BodyTraceEmitter<'a>> {
    (trace_id != 0).then(|| {
        BodyTraceEmitter::new(
            &state.trace,
            trace_id,
            rsproxy_trace::BodyDirection::Response,
            limit,
        )
    })
}

pub(super) fn relay_exact<R: Read + ?Sized, W: Write + ?Sized>(
    reader: &mut R,
    client: &mut W,
    length: usize,
    summary: &mut BodySummary,
    mut trace: Option<&mut BodyTraceEmitter<'_>>,
    deadline: RequestDeadline,
) -> io::Result<()> {
    let mut remaining = length;
    let mut buffer = [0u8; RELAY_BUFFER_SIZE];
    while remaining > 0 {
        deadline.remaining()?;
        let take = remaining.min(buffer.len());
        reader.read_exact(&mut buffer[..take])?;
        client.write_all(&buffer[..take])?;
        summary.observe(&buffer[..take]);
        if let Some(trace) = &mut trace {
            trace.observe_slice(&buffer[..take]);
        }
        remaining -= take;
    }
    Ok(())
}

pub(super) fn relay_to_eof<R: Read + ?Sized, W: Write + ?Sized>(
    reader: &mut R,
    client: &mut W,
    summary: &mut BodySummary,
    mut trace: Option<&mut BodyTraceEmitter<'_>>,
    deadline: RequestDeadline,
) -> io::Result<()> {
    let mut buffer = [0u8; RELAY_BUFFER_SIZE];
    loop {
        deadline.remaining()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        client.write_all(&buffer[..read])?;
        summary.observe(&buffer[..read]);
        if let Some(trace) = &mut trace {
            trace.observe_slice(&buffer[..read]);
        }
    }
}

pub(super) fn relay_chunked<R: Read + ?Sized, W: Write + ?Sized>(
    reader: &mut R,
    client: &mut W,
    summary: &mut BodySummary,
    mut trace: Option<&mut BodyTraceEmitter<'_>>,
    max_header_size: usize,
    max_header_count: usize,
    deadline: RequestDeadline,
) -> io::Result<Vec<(String, String)>> {
    loop {
        deadline.remaining()?;
        let line = read_crlf_line(reader)?;
        let raw_size = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(raw_size, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        if size == 0 {
            return read_trailers(reader, max_header_size, max_header_count);
        }

        let mut remaining = size;
        let mut buffer = [0u8; RELAY_BUFFER_SIZE];
        while remaining > 0 {
            deadline.remaining()?;
            let take = remaining.min(buffer.len());
            reader.read_exact(&mut buffer[..take])?;
            write_chunk(client, &buffer[..take])?;
            summary.observe(&buffer[..take]);
            if let Some(trace) = &mut trace {
                trace.observe_slice(&buffer[..take]);
            }
            remaining -= take;
        }
        let mut delimiter = [0u8; 2];
        reader.read_exact(&mut delimiter)?;
        if delimiter != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk missing trailing CRLF",
            ));
        }
    }
}

fn read_trailers<R: Read + ?Sized>(
    reader: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Vec<(String, String)>> {
    let mut trailers = Vec::new();
    let mut bytes = 0usize;
    loop {
        let line = read_crlf_line(reader)?;
        if line.is_empty() {
            return Ok(trailers);
        }
        bytes = bytes.saturating_add(line.len() + 2);
        if bytes > max_header_size || trailers.len() >= max_header_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response trailer limit exceeded",
            ));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid trailer"))?;
        trailers.push((name.trim().to_string(), value.trim().to_string()));
    }
}

fn write_chunk<W: Write + ?Sized>(client: &mut W, data: &[u8]) -> io::Result<()> {
    let mut encoded = Vec::with_capacity(data.len() + 24);
    write!(encoded, "{:X}\r\n", data.len())?;
    encoded.extend_from_slice(data);
    encoded.extend_from_slice(b"\r\n");
    client.write_all(&encoded)
}

pub(super) fn write_chunk_end<W: Write + ?Sized>(
    client: &mut W,
    trailers: &[(String, String)],
) -> io::Result<()> {
    let mut encoded = Vec::with_capacity(64 + trailers.len() * 32);
    encoded.extend_from_slice(b"0\r\n");
    for (name, value) in trailers {
        write!(encoded, "{name}: {value}\r\n")?;
    }
    encoded.extend_from_slice(b"\r\n");
    client.write_all(&encoded)
}
