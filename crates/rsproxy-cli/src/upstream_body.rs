use crate::transfer_timing::TransferTimer;
use bytes::Bytes;
use std::fmt;
use std::io;
use tokio::sync::mpsc;

const BODY_CHANNEL_CAPACITY: usize = 8;

pub(crate) type UpstreamBodySender = mpsc::Sender<io::Result<UpstreamBodyFrame>>;

#[derive(Debug)]
pub(crate) enum UpstreamBodyFrame {
    Data(Bytes),
    Trailers(Vec<(String, String)>),
}

pub(crate) struct UpstreamBody {
    receiver: mpsc::Receiver<io::Result<UpstreamBodyFrame>>,
    pending: Option<io::Result<UpstreamBodyFrame>>,
    receive_timer: Option<TransferTimer>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CollectedBody {
    pub body: Vec<u8>,
    pub trailers: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BoundedBody {
    Complete(CollectedBody),
    Overflow { prefix: Vec<u8> },
}

impl UpstreamBody {
    #[cfg(test)]
    pub(crate) fn channel() -> (UpstreamBodySender, Self) {
        let (sender, receiver) = mpsc::channel(BODY_CHANNEL_CAPACITY);
        (
            sender,
            Self {
                receiver,
                pending: None,
                receive_timer: None,
            },
        )
    }

    pub(crate) fn timed_channel() -> (UpstreamBodySender, Self, TransferTimer) {
        let (sender, receiver) = mpsc::channel(BODY_CHANNEL_CAPACITY);
        let timer = TransferTimer::start();
        (
            sender,
            Self {
                receiver,
                pending: None,
                receive_timer: Some(timer.clone()),
            },
            timer,
        )
    }

    #[cfg(test)]
    pub(crate) fn from_collected(body: Vec<u8>, trailers: Vec<(String, String)>) -> Self {
        let (sender, stream) = Self::channel();
        if !body.is_empty() {
            sender
                .try_send(Ok(UpstreamBodyFrame::Data(Bytes::from(body))))
                .expect("new body channel has data capacity");
        }
        if !trailers.is_empty() {
            sender
                .try_send(Ok(UpstreamBodyFrame::Trailers(trailers)))
                .expect("new body channel has trailer capacity");
        }
        drop(sender);
        stream
    }

    pub(crate) fn next(&mut self) -> Option<io::Result<UpstreamBodyFrame>> {
        self.pending
            .take()
            .or_else(|| self.receiver.blocking_recv())
    }

    pub(crate) fn receive_ms(&self) -> Option<u64> {
        self.receive_timer
            .as_ref()
            .map(TransferTimer::elapsed_or_current_ms)
    }

    #[cfg(test)]
    pub(crate) fn collect(mut self) -> io::Result<CollectedBody> {
        let mut body = Vec::new();
        let mut trailers = Vec::new();
        while let Some(frame) = self.next() {
            match frame? {
                UpstreamBodyFrame::Data(data) => body.extend_from_slice(&data),
                UpstreamBodyFrame::Trailers(seen) => trailers.extend(seen),
            }
        }
        Ok(CollectedBody { body, trailers })
    }

    pub(crate) fn collect_bounded(&mut self, limit: usize) -> io::Result<BoundedBody> {
        let mut body = Vec::with_capacity(limit.min(64 * 1024));
        let mut trailers = Vec::new();
        while let Some(frame) = self.next() {
            match frame? {
                UpstreamBodyFrame::Data(data) => {
                    let available = limit.saturating_sub(body.len());
                    if data.len() > available {
                        body.extend_from_slice(&data[..available]);
                        let remaining = data.slice(available..);
                        if !remaining.is_empty() {
                            self.pending = Some(Ok(UpstreamBodyFrame::Data(remaining)));
                        }
                        return Ok(BoundedBody::Overflow { prefix: body });
                    }
                    body.extend_from_slice(&data);
                }
                UpstreamBodyFrame::Trailers(seen) => trailers.extend(seen),
            }
        }
        Ok(BoundedBody::Complete(CollectedBody { body, trailers }))
    }
}

impl fmt::Debug for UpstreamBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamBody")
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
