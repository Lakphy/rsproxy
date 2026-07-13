//! Command-line composition root for the rsproxy executable.
//!
//! The facade parses process arguments, resolves CLI/file/default precedence,
//! injects platform-owned resources into the proxy engine, and renders stable
//! human or JSON output. It coordinates the engine, control API, and platform
//! crates but does not implement proxy transport, rule semantics, or OS policy.
//! Applications normally call [`parse_cli`] once and pass the returned
//! [`ParsedCli`] to [`run_parsed`]; [`run_cli`] performs both steps.
mod app;
/// Argument parsing and execution entry points for embedding the rsproxy CLI.
pub mod cli;
mod error;
mod logging;
mod tui;

pub use cli::{ParsedCli, parse_cli, run_cli, run_parsed};
pub use error::{CliError, CliResult, ConfigError, DaemonConflict, LoggingError, RuleDiagnostics};
