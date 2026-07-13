use std::io;
use std::time::{Duration, Instant};

const REQUEST_TOTAL_STAGE: &str = "stage=request_total: timeout after ";

#[derive(Clone, Copy, Debug)]
/// Absolute wall-clock budget for all stages of one request.
///
/// The clock starts in [`RequestDeadline::new`], before any stage-specific work.
pub struct RequestDeadline {
    started: Instant,
    timeout: Duration,
}

#[derive(Clone, Copy, Debug)]
/// Effective timeout for one stage after clipping it to the request deadline.
pub struct TimeoutBudget {
    timeout: Duration,
    request_timeout: Duration,
    request_limited: bool,
}

impl RequestDeadline {
    /// Starts a request-total deadline now and rejects a zero duration.
    pub fn new(timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stage=request_total: timeout must be greater than zero",
            ));
        }
        Ok(Self {
            started: Instant::now(),
            timeout,
        })
    }

    /// Returns wall time remaining since construction, or the total-timeout error.
    pub fn remaining(self) -> io::Result<Duration> {
        self.timeout
            .checked_sub(self.started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| self.timeout_error())
    }

    /// Clips a stage timeout to the request time remaining at this call.
    pub fn budget(self, stage_timeout: Duration) -> io::Result<TimeoutBudget> {
        if stage_timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stage timeout must be greater than zero",
            ));
        }
        let remaining = self.remaining()?;
        Ok(TimeoutBudget {
            timeout: remaining.min(stage_timeout),
            request_timeout: self.timeout,
            request_limited: remaining <= stage_timeout,
        })
    }

    /// Sleeps synchronously without extending the original request deadline.
    pub fn sleep(self, duration: Duration) -> io::Result<()> {
        if duration.is_zero() {
            return self.remaining().map(|_| ());
        }
        let remaining = self.remaining()?;
        if duration >= remaining {
            std::thread::sleep(remaining);
            return Err(self.timeout_error());
        }
        std::thread::sleep(duration);
        self.remaining().map(|_| ())
    }

    /// Creates the canonical timeout error for this request-total budget.
    pub fn timeout_error(self) -> io::Error {
        request_total_timeout_error(self.timeout)
    }
}

impl TimeoutBudget {
    /// Returns the effective duration a stage may wait from budget creation.
    pub fn timeout(self) -> Duration {
        self.timeout
    }

    /// Reports either request-total or stage-local expiration, whichever limited the budget.
    pub fn timeout_error(self, stage_error: impl FnOnce(Duration) -> io::Error) -> io::Error {
        if self.request_limited {
            request_total_timeout_error(self.request_timeout)
        } else {
            stage_error(self.timeout)
        }
    }

    /// Reclassifies a timed-out stage error when the request deadline was limiting.
    pub fn map_timeout(self, error: io::Error) -> io::Error {
        if self.request_limited && is_timeout_kind(&error) {
            request_total_timeout_error(self.request_timeout)
        } else {
            error
        }
    }
}

pub(crate) fn request_total_timeout_error(timeout: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "{REQUEST_TOTAL_STAGE}{}ms",
            timeout.as_millis().min(u64::MAX as u128)
        ),
    )
}

/// Returns whether an I/O error is the canonical request-total timeout marker.
pub fn is_request_total_timeout(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut && error.to_string().starts_with(REQUEST_TOTAL_STAGE)
}

fn is_timeout_kind(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}
