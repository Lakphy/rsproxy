use super::*;

pub(super) struct H2RequestReader {
    receiver: mpsc::Receiver<io::Result<H2RequestFrame>>,
    runtime: Handle,
    pending: Vec<u8>,
    offset: usize,
    timeout: Option<Duration>,
    finished: bool,
}

impl H2RequestReader {
    pub(super) fn new(
        receiver: mpsc::Receiver<io::Result<H2RequestFrame>>,
        runtime: Handle,
    ) -> Self {
        Self {
            receiver,
            runtime,
            pending: Vec::new(),
            offset: 0,
            timeout: None,
            finished: false,
        }
    }

    pub(super) fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    fn refill(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        loop {
            let frame = self.receive()?;
            match frame {
                Some(H2RequestFrame::Data(data)) if data.is_empty() => continue,
                Some(H2RequestFrame::Data(data)) => {
                    self.pending = format!("{:X}\r\n", data.len()).into_bytes();
                    self.pending.extend_from_slice(&data);
                    self.pending.extend_from_slice(b"\r\n");
                    self.offset = 0;
                    return Ok(());
                }
                Some(H2RequestFrame::Trailers(trailers)) => {
                    self.pending = b"0\r\n".to_vec();
                    for (name, value) in trailers {
                        self.pending
                            .extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
                    }
                    self.pending.extend_from_slice(b"\r\n");
                    self.offset = 0;
                    self.finished = true;
                    return Ok(());
                }
                None => {
                    self.pending = b"0\r\n\r\n".to_vec();
                    self.offset = 0;
                    self.finished = true;
                    return Ok(());
                }
            }
        }
    }

    fn receive(&mut self) -> io::Result<Option<H2RequestFrame>> {
        let result = if let Some(timeout) = self.timeout {
            let runtime = self.runtime.clone();
            let receiver = &mut self.receiver;
            runtime
                .block_on(async { tokio::time::timeout(timeout, receiver.recv()).await })
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "HTTP/2 request body read timed out",
                    )
                })?
        } else {
            self.receiver.blocking_recv()
        };
        result.transpose()
    }
}

impl Read for H2RequestReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.offset == self.pending.len() {
            self.pending.clear();
            self.offset = 0;
            self.refill()?;
        }
        if self.pending.is_empty() {
            return Ok(0);
        }
        let size = buffer.len().min(self.pending.len() - self.offset);
        buffer[..size].copy_from_slice(&self.pending[self.offset..self.offset + size]);
        self.offset += size;
        Ok(size)
    }
}
