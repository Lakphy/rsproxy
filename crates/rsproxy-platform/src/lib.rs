// OS process, trust-store, and registry adapters require narrowly scoped FFI calls.
#![allow(unsafe_code)]

//! Operating-system integration for rsproxy.

pub mod ca;
mod error;
pub mod process;
pub mod system_proxy;

pub use error::{PlatformError, PlatformResult};
