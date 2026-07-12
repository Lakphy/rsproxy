mod app;
mod async_io;
#[cfg(feature = "bench-support")]
#[doc(hidden)]
pub mod benchmark_support;
pub mod cli;
mod control;
mod dns;
mod h2;
mod http;
mod json;
mod logging;
mod proxy;
mod request_deadline;
mod rule_store;
mod transfer_timing;
mod tui;
mod upstream_body;
mod upstream_h2;
mod upstream_pool;
#[cfg(windows)]
mod windows_pipe;

pub use cli::run_cli;
