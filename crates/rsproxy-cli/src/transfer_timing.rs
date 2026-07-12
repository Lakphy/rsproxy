use bytes::Bytes;
use http_body::Body;
use http_body_util::BodyExt;
use http_body_util::combinators::BoxBody;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

const UNFINISHED: u64 = u64::MAX;

#[derive(Clone, Debug)]
pub(crate) struct TransferTimer {
    inner: Arc<TransferTimerInner>,
}

#[derive(Debug)]
struct TransferTimerInner {
    started: Instant,
    elapsed_ms: AtomicU64,
}

impl TransferTimer {
    pub(crate) fn start() -> Self {
        Self {
            inner: Arc::new(TransferTimerInner {
                started: Instant::now(),
                elapsed_ms: AtomicU64::new(UNFINISHED),
            }),
        }
    }

    pub(crate) fn finish(&self) -> u64 {
        let elapsed = duration_millis(self.inner.started.elapsed());
        let _ = self.inner.elapsed_ms.compare_exchange(
            UNFINISHED,
            elapsed,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
        self.inner.elapsed_ms.load(Ordering::Acquire)
    }

    pub(crate) fn elapsed_ms(&self) -> Option<u64> {
        match self.inner.elapsed_ms.load(Ordering::Acquire) {
            UNFINISHED => None,
            elapsed => Some(elapsed),
        }
    }

    pub(crate) fn elapsed_or_current_ms(&self) -> u64 {
        self.elapsed_ms()
            .unwrap_or_else(|| duration_millis(self.inner.started.elapsed()))
    }
}

pub(crate) fn timed_body<E>(body: BoxBody<Bytes, E>, timer: TransferTimer) -> BoxBody<Bytes, E>
where
    E: Send + Sync + 'static,
{
    TimedBody::new(body, timer).boxed()
}

struct TimedBody<B> {
    body: B,
    timer: TransferTimer,
}

impl<B> TimedBody<B>
where
    B: Body,
{
    fn new(body: B, timer: TransferTimer) -> Self {
        if body.is_end_stream() {
            timer.finish();
        }
        Self { body, timer }
    }
}

impl<B> Body for TimedBody<B>
where
    B: Body + Unpin,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(context);
        if matches!(frame, Poll::Ready(None))
            || matches!(frame, Poll::Ready(Some(_))) && self.body.is_end_stream()
        {
            self.timer.finish();
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}

impl<B> Drop for TimedBody<B> {
    fn drop(&mut self) {
        self.timer.finish();
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "transfer_timing/tests.rs"]
mod tests;
