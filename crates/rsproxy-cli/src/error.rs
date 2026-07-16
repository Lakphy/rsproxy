use rsproxy_engine::{EngineError, RuleStoreError};
use rsproxy_platform::PlatformError;
use rsproxy_rules::{RuleError, RuleModelError};
use std::fmt;
use std::io;
use std::num::ParseIntError;
use std::path::PathBuf;
use std::process::ExitStatus;
use thiserror::Error;

#[derive(Debug, Error)]
/// Typed failure returned by CLI parsing, composition, or command execution.
pub enum CliError {
    /// Command arguments or requested operation are semantically invalid.
    #[error("{0}")]
    Usage(String),
    /// Clap rejected the command-line syntax.
    #[error(transparent)]
    Clap(#[from] clap::Error),
    /// Configuration loading or validation failed.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The proxy engine failed to initialize or execute.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// A control API request or listener operation failed.
    #[error(transparent)]
    Control(#[from] rsproxy_control::ControlError),
    /// An operating-system integration operation failed.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// A standalone rules model value was invalid.
    #[error(transparent)]
    RuleModel(#[from] RuleModelError),
    /// Persistent rule-group storage failed.
    #[error(transparent)]
    RuleStore(#[from] RuleStoreError),
    /// One or more rule parser diagnostics must be rendered together.
    #[error(transparent)]
    RuleDiagnostics(#[from] RuleDiagnostics),
    /// `rules lint` reported shadowed rules; findings were already printed.
    #[error("{0} shadowed rule(s) found")]
    LintFindings(usize),
    /// Logging configuration was invalid.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// A filesystem, socket, or stream operation failed in a named context.
    #[error("{context}: {source}")]
    Io {
        /// Operation being attempted when the failure occurred.
        context: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// JSON input or output failed in a stable rendering context.
    #[error("{context}: {source}")]
    Json {
        /// Static operation label exposed in diagnostics.
        context: &'static str,
        /// Underlying JSON codec failure.
        #[source]
        source: serde_json::Error,
    },
    /// Daemon state made the requested lifecycle transition unsafe.
    #[error(transparent)]
    DaemonConflict(#[from] DaemonConflict),
    /// A required platform helper command returned a non-success status.
    #[error("external command `{command}` exited with {status}")]
    ExternalCommand {
        /// Rendered command identifier safe to show to the user.
        command: String,
        /// Process exit status returned by the operating system.
        status: ExitStatus,
    },
    /// A serving thread returned even though listeners are expected to be long-lived.
    #[error("{listener} listener stopped unexpectedly")]
    ListenerStopped {
        /// Stable listener name used for diagnostics and error classification.
        listener: &'static str,
    },
    /// The channel supervising listener threads disconnected.
    #[error("listener supervision channel disconnected: {source}")]
    ListenerSupervision {
        /// Underlying channel receive failure.
        #[source]
        source: std::sync::mpsc::RecvError,
    },
    /// A newly spawned daemon exited before reporting readiness.
    #[error("rsproxy exited during start with status {status}; see {}", log_path.display())]
    DaemonExited {
        /// Early daemon process exit status.
        status: ExitStatus,
        /// Log file containing daemon startup diagnostics.
        log_path: PathBuf,
    },
    /// A daemon process stayed alive but did not become ready before the startup deadline.
    #[error("rsproxy did not become ready; pid={pid} log={}", log_path.display())]
    DaemonReadinessTimeout {
        /// Spawned daemon process identifier.
        pid: u32,
        /// Log file containing daemon startup diagnostics.
        log_path: PathBuf,
    },
    /// A daemon remained alive after the bounded shutdown wait.
    #[error("pid {pid} did not stop in time")]
    DaemonStopTimeout {
        /// Daemon process identifier that did not terminate.
        pid: u32,
    },
    /// The foreground process observed its supervising launcher exit and shut down cleanly.
    ///
    /// Signalled internally through the listener channel; `run` treats it as a successful exit.
    #[error("supervising process exited")]
    SupervisorExited,
    /// A configured port is held by a process that is not this rsproxy build.
    #[error("{addr} is held by process {pid}, which is not rsproxy; stop it manually")]
    PortHeldByForeignProcess {
        /// Address that could not be reclaimed.
        addr: String,
        /// Process identifier currently holding the address.
        pid: u32,
    },
    /// A platform adapter returned a typed state invalid for the requested command.
    #[error("invalid platform outcome: {detail}")]
    InvalidPlatformOutcome {
        /// Static invariant description suitable for stable JSON output.
        detail: &'static str,
    },
    /// Internal rule command dispatch reached an unsupported operation.
    #[error("invalid internal rule operation")]
    InvalidRuleOperation,
}

/// Result alias used by every executable composition-root operation.
pub type CliResult<T> = Result<T, CliError>;

impl CliError {
    /// Returns the stable machine-readable category used by JSON error output.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) | Self::Clap(_) => "usage_error",
            Self::Config(_) => "config_error",
            Self::Engine(_) | Self::ListenerStopped { .. } | Self::ListenerSupervision { .. } => {
                "engine_error"
            }
            Self::Control(_) => "control_error",
            Self::Platform(_) => "platform_error",
            Self::RuleModel(_) | Self::RuleStore(_) | Self::RuleDiagnostics(_) => "rule_error",
            Self::LintFindings(_) => "rule_error",
            Self::Io { .. } => "io_error",
            Self::Json { .. } => "json_error",
            Self::DaemonConflict(_) => "daemon_conflict",
            Self::ExternalCommand { .. } => "external_command_failed",
            Self::Logging(_) => "config_error",
            Self::DaemonExited { .. }
            | Self::DaemonReadinessTimeout { .. }
            | Self::DaemonStopTimeout { .. }
            | Self::SupervisorExited => "daemon_error",
            Self::PortHeldByForeignProcess { .. } => "daemon_conflict",
            Self::InvalidPlatformOutcome { .. } => "platform_error",
            Self::InvalidRuleOperation => "rule_error",
        }
    }

    /// Returns the process exit status assigned to this error category.
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) | Self::Clap(_) => 2,
            Self::DaemonConflict(_) | Self::PortHeldByForeignProcess { .. } => 3,
            _ => 1,
        }
    }

    /// Wraps an I/O failure with the operation that was being attempted.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

#[derive(Debug, Error)]
/// Failure while loading, merging, or validating runtime configuration.
pub enum ConfigError {
    /// A selected TOML configuration file could not be read.
    #[error("read config {}: {source}", path.display())]
    Read {
        /// Selected configuration path.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Configuration text was not valid for the supported schema.
    #[error("parse config {}: {source}", path.display())]
    Parse {
        /// Selected configuration path.
        path: PathBuf,
        /// Underlying TOML decoder failure.
        #[source]
        source: toml::de::Error,
    },
    /// A parsed value violated a cross-field or range invariant.
    #[error("invalid configuration: {0}")]
    Invalid(String),
    /// An integer-valued option could not be parsed.
    #[error("invalid {field} `{input}`: {source}")]
    InvalidInteger {
        /// Stable option or configuration-field name.
        field: &'static str,
        /// Original rejected input.
        input: String,
        /// Underlying integer parser failure.
        #[source]
        source: ParseIntError,
    },
}

#[derive(Debug, Error)]
/// Failure while translating environment variables into logging settings.
pub enum LoggingError {
    /// The tracing filter expression was syntactically invalid.
    #[error("invalid log filter `{filter}`: {source}")]
    InvalidFilter {
        /// Original rejected filter expression.
        filter: String,
        /// Underlying tracing filter parser failure.
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },
    /// The selected log renderer was neither text nor JSON.
    #[error("invalid RSPROXY_LOG_FORMAT `{value}`; expected text or json")]
    InvalidFormat {
        /// Original rejected renderer name.
        value: String,
    },
}

#[derive(Debug, Error)]
/// Conflict between a requested daemon lifecycle action and observed process state.
pub enum DaemonConflict {
    /// A verified daemon already owns the configured runtime files.
    #[error("rsproxy already running with pid {pid} ({})", pid_path.display())]
    AlreadyRunning {
        /// Verified live daemon process identifier.
        pid: u32,
        /// Pidfile that recorded the identifier.
        pid_path: PathBuf,
    },
    /// No pidfile exists for a stop or status operation.
    #[error("pidfile not found: {}", pid_path.display())]
    NotRunning {
        /// Expected pidfile path.
        pid_path: PathBuf,
    },
    /// A pidfile names a live process that cannot be verified as this daemon.
    #[error(
        "pidfile {} references live process {pid}, but daemon identity could not be verified; refusing to {operation}",
        pid_path.display()
    )]
    IdentityMismatch {
        /// Live process identifier found in the pidfile.
        pid: u32,
        /// Pidfile containing the unverified identifier.
        pid_path: PathBuf,
        /// Destructive operation refused by the identity check.
        operation: &'static str,
    },
}

#[derive(Debug)]
/// Ordered rule parser failures rendered as one multiline CLI diagnostic.
pub struct RuleDiagnostics(pub Vec<RuleError>);

impl fmt::Display for RuleDiagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RuleDiagnostics {}

#[cfg(test)]
#[path = "error/tests.rs"]
mod tests;
