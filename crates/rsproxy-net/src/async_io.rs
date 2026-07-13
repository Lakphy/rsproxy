use rustls::{ClientConnection, ServerConnection, StreamOwned};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
#[cfg(unix)]
use tokio::io::unix::AsyncFd;

#[cfg(not(unix))]
use std::future::Future;
#[cfg(not(unix))]
use std::time::Duration;
#[cfg(not(unix))]
use tokio::time::{Sleep, sleep};

#[cfg(not(unix))]
const IO_RETRY_DELAY: Duration = Duration::from_millis(1);

/// Blocking stream operations that [`AsyncIo`] can drive from Tokio readiness.
pub trait ReadyIo: Read + Write + Send + Unpin + 'static {
    /// Switches the underlying transport between blocking and nonblocking mode.
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()>;
    /// Starts protocol-level shutdown, such as emitting a TLS close notification.
    fn begin_shutdown(&mut self);
    /// Closes the transport's write half after pending protocol bytes are flushed.
    fn shutdown_write(&mut self) -> io::Result<()>;

    #[cfg(unix)]
    /// Returns the descriptor registered with Tokio's readiness reactor.
    fn raw_fd(&self) -> RawFd;
}

impl ReadyIo for TcpStream {
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        TcpStream::set_nonblocking(self, nonblocking)
    }

    fn begin_shutdown(&mut self) {}

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.shutdown(Shutdown::Write)
    }

    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        self.as_raw_fd()
    }
}

impl ReadyIo for StreamOwned<ClientConnection, TcpStream> {
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.sock.set_nonblocking(nonblocking)
    }

    fn begin_shutdown(&mut self) {
        self.conn.send_close_notify();
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.sock.shutdown(Shutdown::Write)
    }

    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }
}

impl ReadyIo for StreamOwned<ServerConnection, TcpStream> {
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.sock.set_nonblocking(nonblocking)
    }

    fn begin_shutdown(&mut self) {
        self.conn.send_close_notify();
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.sock.shutdown(Shutdown::Write)
    }

    #[cfg(unix)]
    fn raw_fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }
}

#[cfg(unix)]
struct ReadyFd<S: ReadyIo>(S);

#[cfg(unix)]
impl<S: ReadyIo> AsRawFd for ReadyFd<S> {
    fn as_raw_fd(&self) -> RawFd {
        self.0.raw_fd()
    }
}

#[cfg(unix)]
/// Adapts a blocking TCP or TLS stream to Tokio's asynchronous I/O traits.
pub struct AsyncIo<S: ReadyIo> {
    inner: AsyncFd<ReadyFd<S>>,
    shutdown_started: bool,
}

#[cfg(unix)]
impl<S: ReadyIo> AsyncIo<S> {
    /// Enables nonblocking mode and registers `inner` with the Tokio reactor.
    pub fn new(mut inner: S) -> io::Result<Self> {
        inner.set_nonblocking(true)?;
        Ok(Self {
            inner: AsyncFd::new(ReadyFd(inner))?,
            shutdown_started: false,
        })
    }
}

#[cfg(unix)]
impl<S: ReadyIo> AsyncRead for AsyncIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let this = self.get_mut();
        loop {
            let mut guard = match this.inner.poll_read_ready_mut(cx) {
                Poll::Ready(Ok(guard)) => guard,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            match guard.try_io(|fd| fd.get_mut().0.read(buf.initialize_unfilled())) {
                Ok(Ok(read)) => {
                    buf.advance(read);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}

#[cfg(unix)]
impl<S: ReadyIo> AsyncWrite for AsyncIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let this = self.get_mut();
        loop {
            let mut guard = match this.inner.poll_write_ready_mut(cx) {
                Poll::Ready(Ok(guard)) => guard,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            match guard.try_io(|fd| fd.get_mut().0.write(buf)) {
                Ok(Ok(written)) => return Poll::Ready(Ok(written)),
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        loop {
            let mut guard = match this.inner.poll_write_ready_mut(cx) {
                Poll::Ready(Ok(guard)) => guard,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            match guard.try_io(|fd| fd.get_mut().0.flush()) {
                Ok(Ok(())) => return Poll::Ready(Ok(())),
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        if !self.shutdown_started {
            self.inner.get_mut().0.begin_shutdown();
            self.shutdown_started = true;
        }
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(self.inner.get_mut().0.shutdown_write()),
            other => other,
        }
    }
}

#[cfg(not(unix))]
/// Adapts a blocking TCP or TLS stream to Tokio's asynchronous I/O traits.
pub struct AsyncIo<S: ReadyIo> {
    inner: S,
    read_wait: Option<Pin<Box<Sleep>>>,
    write_wait: Option<Pin<Box<Sleep>>>,
    shutdown_started: bool,
}

#[cfg(not(unix))]
impl<S: ReadyIo> AsyncIo<S> {
    /// Enables nonblocking mode and initializes readiness-retry state.
    pub fn new(mut inner: S) -> io::Result<Self> {
        inner.set_nonblocking(true)?;
        Ok(Self {
            inner,
            read_wait: None,
            write_wait: None,
            shutdown_started: false,
        })
    }
}

#[cfg(not(unix))]
impl<S: ReadyIo> AsyncRead for AsyncIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            match this.inner.read(buf.initialize_unfilled()) {
                Ok(read) => {
                    buf.advance(read);
                    this.read_wait = None;
                    return Poll::Ready(Ok(()));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if poll_retry_wait(&mut this.read_wait, cx).is_pending() {
                        return Poll::Pending;
                    }
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }
}

#[cfg(not(unix))]
impl<S: ReadyIo> AsyncWrite for AsyncIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        loop {
            match this.inner.write(buf) {
                Ok(written) => {
                    this.write_wait = None;
                    return Poll::Ready(Ok(written));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if poll_retry_wait(&mut this.write_wait, cx).is_pending() {
                        return Poll::Pending;
                    }
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        loop {
            match this.inner.flush() {
                Ok(()) => {
                    this.write_wait = None;
                    return Poll::Ready(Ok(()));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if poll_retry_wait(&mut this.write_wait, cx).is_pending() {
                        return Poll::Pending;
                    }
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        if !self.shutdown_started {
            self.inner.begin_shutdown();
            self.shutdown_started = true;
        }
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(self.inner.shutdown_write()),
            other => other,
        }
    }
}

#[cfg(not(unix))]
fn poll_retry_wait(wait: &mut Option<Pin<Box<Sleep>>>, cx: &mut Context<'_>) -> Poll<()> {
    let timer = wait.get_or_insert_with(|| Box::pin(sleep(IO_RETRY_DELAY)));
    match timer.as_mut().poll(cx) {
        Poll::Ready(()) => {
            *wait = None;
            Poll::Ready(())
        }
        Poll::Pending => Poll::Pending,
    }
}
