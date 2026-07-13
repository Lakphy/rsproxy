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
pub enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Clap(#[from] clap::Error),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Control(#[from] rsproxy_control::ControlError),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    RuleModel(#[from] RuleModelError),
    #[error(transparent)]
    RuleStore(#[from] RuleStoreError),
    #[error(transparent)]
    RuleDiagnostics(#[from] RuleDiagnostics),
    #[error(transparent)]
    Logging(#[from] LoggingError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("{context}: {source}")]
    Json {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    DaemonConflict(#[from] DaemonConflict),
    #[error("external command `{command}` exited with {status}")]
    ExternalCommand { command: String, status: ExitStatus },
    #[error("{listener} listener stopped unexpectedly")]
    ListenerStopped { listener: &'static str },
    #[error("listener supervision channel disconnected: {source}")]
    ListenerSupervision {
        #[source]
        source: std::sync::mpsc::RecvError,
    },
    #[error("rsproxy exited during start with status {status}; see {}", log_path.display())]
    DaemonExited {
        status: ExitStatus,
        log_path: PathBuf,
    },
    #[error("rsproxy did not become ready; pid={pid} log={}", log_path.display())]
    DaemonReadinessTimeout { pid: u32, log_path: PathBuf },
    #[error("pid {pid} did not stop in time")]
    DaemonStopTimeout { pid: u32 },
    #[error("invalid platform outcome: {detail}")]
    InvalidPlatformOutcome { detail: &'static str },
    #[error("invalid internal rule operation")]
    InvalidRuleOperation,
}

pub type CliResult<T> = Result<T, CliError>;

impl CliError {
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
            Self::Io { .. } => "io_error",
            Self::Json { .. } => "json_error",
            Self::DaemonConflict(_) => "daemon_conflict",
            Self::ExternalCommand { .. } => "external_command_failed",
            Self::Logging(_) => "config_error",
            Self::DaemonExited { .. }
            | Self::DaemonReadinessTimeout { .. }
            | Self::DaemonStopTimeout { .. } => "daemon_error",
            Self::InvalidPlatformOutcome { .. } => "platform_error",
            Self::InvalidRuleOperation => "rule_error",
        }
    }

    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) | Self::Clap(_) => 2,
            Self::DaemonConflict(_) => 3,
            _ => 1,
        }
    }

    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("parse config {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("invalid {field} `{input}`: {source}")]
    InvalidInteger {
        field: &'static str,
        input: String,
        #[source]
        source: ParseIntError,
    },
}

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("invalid log filter `{filter}`: {source}")]
    InvalidFilter {
        filter: String,
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },
    #[error("invalid RSPROXY_LOG_FORMAT `{value}`; expected text or json")]
    InvalidFormat { value: String },
}

#[derive(Debug, Error)]
pub enum DaemonConflict {
    #[error("rsproxy already running with pid {pid} ({})", pid_path.display())]
    AlreadyRunning { pid: u32, pid_path: PathBuf },
    #[error("pidfile not found: {}", pid_path.display())]
    NotRunning { pid_path: PathBuf },
    #[error(
        "pidfile {} references live process {pid}, but daemon identity could not be verified; refusing to {operation}",
        pid_path.display()
    )]
    IdentityMismatch {
        pid: u32,
        pid_path: PathBuf,
        operation: &'static str,
    },
}

#[derive(Debug)]
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
