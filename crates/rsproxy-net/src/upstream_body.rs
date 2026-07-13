use crate::transfer_timing::TransferTimer;
use bytes::Bytes;
use std::fmt;
use std::io;
use tokio::sync::mpsc;

const BODY_CHANNEL_CAPACITY: usize = 8;

pub(crate) type UpstreamBodySender = mpsc::Sender<io::Result<UpstreamBodyFrame>>;

#[cfg(feature = "test-support")]
/// Test-only handle for reading elapsed upstream body receive time.
pub struct TestReceiveTimer(TransferTimer);

#[cfg(feature = "test-support")]
impl TestReceiveTimer {
    /// Stops the timer if necessary and returns elapsed milliseconds.
    pub fn finish(&self) -> u64 {
        self.0.finish()
    }
}

#[cfg(feature = "test-support")]
/// Creates a test body channel whose receive duration can be inspected.
pub fn test_timed_upstream_body_channel() -> (
    mpsc::Sender<io::Result<UpstreamBodyFrame>>,
    UpstreamBody,
    TestReceiveTimer,
) {
    let (sender, body, timer) = UpstreamBody::timed_channel();
    (sender, body, TestReceiveTimer(timer))
}

#[derive(Debug)]
/// One ordered frame received from an upstream response body.
pub enum UpstreamBodyFrame {
    /// A non-empty body data fragment.
    Data(Bytes),
    /// Terminal response trailers.
    Trailers(Vec<(String, String)>),
}

/// Blocking consumer for the bounded channel driven by an upstream protocol task.
pub struct UpstreamBody {
    receiver: mpsc::Receiver<io::Result<UpstreamBodyFrame>>,
    pending: Option<io::Result<UpstreamBodyFrame>>,
    receive_timer: Option<TransferTimer>,
}

#[derive(Debug, PartialEq, Eq)]
/// A fully buffered upstream body and its terminal trailers.
pub struct CollectedBody {
    /// Concatenated data-frame bytes.
    pub body: Vec<u8>,
    /// Trailer fields in received order.
    pub trailers: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
/// Result of buffering an upstream body under a byte limit.
pub enum BoundedBody {
    /// The entire body and trailers fit within the limit.
    Complete(CollectedBody),
    /// The limit was reached while unread frames remain on the body stream.
    Overflow {
        /// At most `limit` leading body bytes retained for inspection.
        prefix: Vec<u8>,
    },
}

impl UpstreamBody {
    #[cfg(any(test, feature = "test-support"))]
    /// Creates a test-support channel without receive-time instrumentation.
    pub fn channel() -> (mpsc::Sender<io::Result<UpstreamBodyFrame>>, Self) {
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

    #[cfg(any(test, feature = "test-support"))]
    /// Creates a closed test body stream from already collected data.
    pub fn from_collected(body: Vec<u8>, trailers: Vec<(String, String)>) -> Self {
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

    /// Returns milliseconds since timed channel creation, if timing is enabled.
    pub fn receive_ms(&self) -> Option<u64> {
        self.receive_timer
            .as_ref()
            .map(TransferTimer::elapsed_or_current_ms)
    }

    #[allow(clippy::should_implement_trait)]
    /// Blocks until the next frame arrives or all senders have closed.
    pub fn next(&mut self) -> Option<io::Result<UpstreamBodyFrame>> {
        self.pending
            .take()
            .or_else(|| self.receiver.blocking_recv())
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Drains every frame into memory without imposing a byte limit.
    pub fn collect(mut self) -> io::Result<CollectedBody> {
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

    /// Buffers up to `limit` bytes while preserving unread overflow for later calls.
    pub fn collect_bounded(&mut self, limit: usize) -> io::Result<BoundedBody> {
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
