use super::*;

pub(super) struct DeadlineIo<'a> {
    stream: &'a mut UpstreamStream,
    deadline: RequestDeadline,
}

impl<'a> DeadlineIo<'a> {
    pub(super) fn new(stream: &'a mut UpstreamStream, deadline: RequestDeadline) -> Self {
        Self { stream, deadline }
    }

    fn prepare_read(&mut self) -> io::Result<TimeoutBudget> {
        let budget = self.deadline.budget(UPSTREAM_READ_TIMEOUT)?;
        self.stream
            .set_io_timeouts(Some(budget.timeout()), Some(UPSTREAM_WRITE_TIMEOUT))?;
        Ok(budget)
    }

    fn prepare_write(&mut self) -> io::Result<TimeoutBudget> {
        let budget = self.deadline.budget(UPSTREAM_WRITE_TIMEOUT)?;
        self.stream
            .set_io_timeouts(Some(UPSTREAM_READ_TIMEOUT), Some(budget.timeout()))?;
        Ok(budget)
    }
}

impl Read for DeadlineIo<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let budget = self.prepare_read()?;
        self.stream
            .read(buf)
            .map_err(|error| budget.map_timeout(error))
    }
}

impl Write for DeadlineIo<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let budget = self.prepare_write()?;
        self.stream
            .write(buf)
            .map_err(|error| budget.map_timeout(error))
    }

    fn flush(&mut self) -> io::Result<()> {
        let budget = self.prepare_write()?;
        self.stream
            .flush()
            .map_err(|error| budget.map_timeout(error))
    }
}

pub(super) fn restore_upstream_timeouts(stream: &mut UpstreamStream) -> io::Result<()> {
    stream.set_io_timeouts(Some(UPSTREAM_READ_TIMEOUT), Some(UPSTREAM_WRITE_TIMEOUT))
}
