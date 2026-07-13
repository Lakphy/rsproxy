use super::{DownstreamH2Body, DownstreamH2ResponseFrame, DownstreamH2ResponseHead};
use bytes::Bytes;
use http_body::Body;
use http_body_util::BodyExt;
use hyper::HeaderMap;
use hyper::body::Frame;
use hyper::header::{HeaderName, HeaderValue};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

pub(super) fn channel_body(
    receiver: mpsc::Receiver<io::Result<DownstreamH2ResponseFrame>>,
) -> DownstreamH2Body {
    DownstreamH2ChannelBody { receiver }.boxed()
}

pub(super) fn response_parts(head: DownstreamH2ResponseHead) -> io::Result<(u16, HeaderMap)> {
    let mut headers = HeaderMap::new();
    append_headers(&mut headers, head.headers, "response")?;
    Ok((head.status, headers))
}

struct DownstreamH2ChannelBody {
    receiver: mpsc::Receiver<io::Result<DownstreamH2ResponseFrame>>,
}

impl Body for DownstreamH2ChannelBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.receiver).poll_recv(context) {
            Poll::Ready(Some(Ok(DownstreamH2ResponseFrame::Data(data)))) => {
                Poll::Ready(Some(Ok(Frame::data(data))))
            }
            Poll::Ready(Some(Ok(DownstreamH2ResponseFrame::Trailers(trailers)))) => {
                Poll::Ready(Some(trailer_map(trailers).map(Frame::trailers)))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.receiver.is_closed() && self.receiver.is_empty()
    }
}

fn trailer_map(trailers: Vec<(String, String)>) -> io::Result<HeaderMap> {
    let mut output = HeaderMap::new();
    append_headers(&mut output, trailers, "trailer")?;
    Ok(output)
}

fn append_headers(
    output: &mut HeaderMap,
    headers: Vec<(String, String)>,
    kind: &str,
) -> io::Result<()> {
    for (name, value) in headers {
        if h2_forbidden_header(&name) {
            continue;
        }
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid bridged {kind} header name: {error}"),
            )
        })?;
        let value = HeaderValue::from_bytes(value.as_bytes()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid bridged {kind} header value: {error}"),
            )
        })?;
        output.append(name, value);
    }
    Ok(())
}

fn h2_forbidden_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
    )
}
