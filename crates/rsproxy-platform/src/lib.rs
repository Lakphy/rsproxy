// OS process, trust-store, and registry adapters require narrowly scoped FFI calls.
#![allow(unsafe_code)]

//! Operating-system integration for rsproxy.
//!
//! This crate owns the narrow boundary where rsproxy reads or mutates host state: persistent CA
//! material, operating-system trust stores, daemon process controls, resident-memory inspection,
//! local control-socket naming, desktop login startup registration, and system-proxy
//! configuration. Planning APIs expose the commands and state changes before execution so callers
//! can implement dry runs and reviewable output.
//!
//! The crate does not implement proxy traffic handling, certificate issuance on the MITM data
//! path, CLI presentation, or privilege escalation. Trust-store and system-proxy mutations use
//! platform commands or APIs and can require the caller to already have administrator privileges.

/// Root-CA generation, persistent certificate storage, and host trust-store operations.
pub mod ca;
mod error;
/// Cross-platform daemon process controls and local control-socket path selection.
pub mod process;
/// Per-user desktop login startup registration.
pub mod startup;
/// Planning, execution, and inspection of desktop operating-system proxy settings.
pub mod system_proxy;

pub use error::{PlatformError, PlatformResult};
