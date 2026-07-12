use crate::async_io::ReadyIo;
use rustls::{ServerConnection, StreamOwned};
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::OnceLock;
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};

pub(crate) fn h2_runtime() -> io::Result<&'static Runtime> {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = RuntimeBuilder::new_multi_thread()
        .enable_io()
        .enable_time()
        .thread_name("rsproxy-h2")
        .build()
        .map_err(io::Error::other)?;
    let _ = RUNTIME.set(runtime);
    Ok(RUNTIME.get().expect("HTTP/2 runtime was initialized"))
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
