use std::io;
use std::sync::OnceLock;
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

/// Returns the process-wide Tokio runtime used by synchronous HTTP/2 and DNS facades.
pub fn h2_runtime() -> io::Result<&'static Runtime> {
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
