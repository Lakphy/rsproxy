use super::*;
use bytes::Bytes;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

mod request;
mod response;

use request::H2RequestReader;
use response::H2ResponseWriter;

const H2_BRIDGE_CHANNEL_CAPACITY: usize = 8;

pub(crate) type H2RequestSender = mpsc::Sender<io::Result<H2RequestFrame>>;

#[derive(Debug)]
pub(crate) enum H2RequestFrame {
    Data(Bytes),
    Trailers(Vec<(String, String)>),
}

#[derive(Debug)]
pub(crate) struct H2BridgeHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) enum H2BridgeFrame {
    Data(Bytes),
    Trailers(Vec<(String, String)>),
}

pub(crate) struct H2BridgeOutput {
    pub head: oneshot::Receiver<io::Result<H2BridgeHead>>,
    pub body: mpsc::Receiver<io::Result<H2BridgeFrame>>,
}

pub(crate) struct H2BridgeIo {
    request: H2RequestReader,
    response: H2ResponseWriter,
}

impl H2BridgeIo {
    pub(crate) fn new(
        request: mpsc::Receiver<io::Result<H2RequestFrame>>,
        runtime: Handle,
        method: &str,
        max_header_size: usize,
        max_header_count: usize,
    ) -> (Self, H2BridgeOutput) {
        let (response, output) = H2ResponseWriter::new(
            method,
            max_header_size,
            max_header_count,
            H2_BRIDGE_CHANNEL_CAPACITY,
        );
        (
            Self {
                request: H2RequestReader::new(request, runtime),
                response,
            },
            output,
        )
    }

    fn finish_response(&mut self) -> io::Result<()> {
        self.response.finish()
    }

    fn fail_response(&mut self, error: &io::Error) {
        self.response.fail_external(error);
    }
}

impl Read for H2BridgeIo {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.request.read(buffer)
    }
}

impl Write for H2BridgeIo {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.response.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.response.flush()
    }
}

impl WsIo for H2BridgeIo {
    fn set_ws_nonblocking(&mut self, _nonblocking: bool) -> io::Result<()> {
        Ok(())
    }

    fn shutdown_ws(&mut self, _how: Shutdown) -> io::Result<()> {
        Ok(())
    }

    fn set_request_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.request.set_timeout(timeout);
        Ok(())
    }
}

pub(crate) fn process_h2_request(
    head: http::RequestHead,
    mut bridge: H2BridgeIo,
    state: SharedState,
    peer: String,
    authority: String,
    client_tls: TlsRecord,
    initial_flags: Vec<String>,
) -> io::Result<()> {
    let request_version = head.request.version.clone();
    let result = match handle_http_head(
        &mut bridge,
        head,
        &state,
        HttpConnectionInput {
            peer,
            https_authority: Some(authority),
            plain_client_clone: None,
            initial_tls: vec![client_tls],
            started_ms_override: None,
            initial_flags,
            client_connection: ClientPersistence::Close,
        },
    ) {
        Err(error) if is_h1_request_input_error(&error) => {
            write_h1_request_input_error(&mut bridge, &request_version, &error)
        }
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        bridge.fail_response(&error);
        return Err(error);
    }
    bridge.finish_response()
}

pub(super) fn prepare_h2_client_response_headers(
    headers: &mut Vec<(String, String)>,
    status: u16,
    body_len: Option<usize>,
) {
    strip_hop_by_hop_headers(headers);
    if let Some(body_len) = body_len
        && !(100..200).contains(&status)
        && !matches!(status, 204 | 304)
        && http::header(headers, "content-length").is_none()
    {
        http::set_header(headers, "Content-Length", body_len.to_string());
    }
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct CapturedHttpResponse {
    pub(super) bytes: Vec<u8>,
}

#[cfg(test)]
impl Read for CapturedHttpResponse {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

#[cfg(test)]
impl Write for CapturedHttpResponse {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl WsIo for CapturedHttpResponse {
    fn set_ws_nonblocking(&mut self, _nonblocking: bool) -> io::Result<()> {
        Ok(())
    }

    fn shutdown_ws(&mut self, _how: Shutdown) -> io::Result<()> {
        Ok(())
    }

    fn set_request_read_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn process_h2_request_collected(
    request: RawRequest,
    state: SharedState,
    peer: String,
    authority: String,
    client_tls: TlsRecord,
    initial_flags: Vec<String>,
) -> io::Result<(H2BridgeHead, Vec<H2BridgeFrame>)> {
    let runtime = crate::h2::h2_runtime()?;
    let (_sender, receiver) = mpsc::channel(1);
    let (bridge, mut output) = H2BridgeIo::new(
        receiver,
        runtime.handle().clone(),
        &request.method,
        state.config.max_header_size,
        state.config.max_header_count,
    );
    let head = http::RequestHead {
        request,
        body: http::RequestBodyFraming::None,
    };
    let worker = thread::spawn(move || {
        process_h2_request(
            head,
            bridge,
            state,
            peer,
            authority,
            client_tls,
            initial_flags,
        )
    });
    let collected = runtime.block_on(async move {
        let head = output
            .head
            .await
            .map_err(|_| io::Error::other("bridge response head channel closed"))??;
        let mut frames = Vec::new();
        while let Some(frame) = output.body.recv().await {
            frames.push(frame?);
        }
        Ok::<_, io::Error>((head, frames))
    });
    worker
        .join()
        .map_err(|_| io::Error::other("bridge worker panicked"))??;
    collected
}

#[cfg(test)]
#[path = "h2_bridge/tests/mod.rs"]
mod tests;
