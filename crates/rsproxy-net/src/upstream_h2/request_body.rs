use super::*;
use http_body::Body;
use http_body_util::BodyExt;
use hyper::HeaderMap;
use hyper::body::Frame;
use hyper::header::{HeaderName, HeaderValue};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

const REQUEST_BODY_CHANNEL_CAPACITY: usize = 8;

enum RequestBodyFrame {
    Data(Bytes),
    Trailers(HeaderMap),
}

#[derive(Clone)]
pub(super) struct H2RequestBodySender {
    sender: mpsc::Sender<io::Result<RequestBodyFrame>>,
    max_header_size: usize,
    max_header_count: usize,
}

impl H2RequestBodySender {
    pub(super) fn send_data(&self, data: Bytes, deadline: RequestDeadline) -> io::Result<bool> {
        self.send(Ok(RequestBodyFrame::Data(data)), deadline)
    }

    pub(super) fn send_trailers(
        &self,
        trailers: Vec<(String, String)>,
        deadline: RequestDeadline,
    ) -> io::Result<bool> {
        crate::http::validate_request_trailers(
            &trailers,
            self.max_header_size,
            self.max_header_count,
        )
        .map_err(|error| stage_error("request_trailer", error))?;
        self.send(
            Ok(RequestBodyFrame::Trailers(trailer_map(trailers)?)),
            deadline,
        )
    }

    pub(super) fn send_error(
        &self,
        error: &io::Error,
        deadline: RequestDeadline,
    ) -> io::Result<bool> {
        self.send(
            Err(io::Error::new(error.kind(), error.to_string())),
            deadline,
        )
    }

    fn send(
        &self,
        frame: io::Result<RequestBodyFrame>,
        deadline: RequestDeadline,
    ) -> io::Result<bool> {
        let timeout = deadline.remaining()?;
        match h2_runtime()?
            .block_on(async { tokio::time::timeout(timeout, self.sender.send(frame)).await })
        {
            Ok(Ok(())) => Ok(true),
            Ok(Err(_)) => Ok(false),
            Err(_) => Err(deadline.timeout_error()),
        }
    }
}

pub(super) fn request_body_channel(
    max_header_size: usize,
    max_header_count: usize,
) -> (H2RequestBodySender, RequestBody) {
    let (sender, receiver) = mpsc::channel(REQUEST_BODY_CHANNEL_CAPACITY);
    (
        H2RequestBodySender {
            sender,
            max_header_size,
            max_header_count,
        },
        ChannelRequestBody {
            receiver,
            finished: false,
        }
        .boxed(),
    )
}

struct ChannelRequestBody {
    receiver: mpsc::Receiver<io::Result<RequestBodyFrame>>,
    finished: bool,
}

impl Body for ChannelRequestBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.receiver).poll_recv(context) {
            Poll::Ready(Some(Ok(RequestBodyFrame::Data(data)))) => {
                Poll::Ready(Some(Ok(Frame::data(data))))
            }
            Poll::Ready(Some(Ok(RequestBodyFrame::Trailers(trailers)))) => {
                self.finished = true;
                self.receiver.close();
                Poll::Ready(Some(Ok(Frame::trailers(trailers))))
            }
            Poll::Ready(Some(Err(error))) => {
                self.finished = true;
                self.receiver.close();
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.finished || (self.receiver.is_closed() && self.receiver.is_empty())
    }
}

fn trailer_map(trailers: Vec<(String, String)>) -> io::Result<HeaderMap> {
    let mut output = HeaderMap::new();
    for (name, value) in trailers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| stage_error("request_trailer", error))?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| stage_error("request_trailer", error))?;
        output.append(name, value);
    }
    Ok(output)
}
