use crate::model::Session;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

pub struct TraceFollow {
    pub(super) backlog: VecDeque<Arc<Session>>,
    pub(super) receiver: Receiver<Arc<Session>>,
    pub(super) _liveness: Arc<()>,
}

impl fmt::Debug for TraceFollow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TraceFollow")
            .field("backlog", &self.backlog.len())
            .finish_non_exhaustive()
    }
}

impl TraceFollow {
    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<Arc<Session>, RecvTimeoutError> {
        if let Some(session) = self.backlog.pop_front() {
            return Ok(session);
        }
        self.receiver.recv_timeout(timeout)
    }

    pub fn try_recv(&mut self) -> Result<Arc<Session>, TryRecvError> {
        if let Some(session) = self.backlog.pop_front() {
            return Ok(session);
        }
        self.receiver.try_recv()
    }
}
