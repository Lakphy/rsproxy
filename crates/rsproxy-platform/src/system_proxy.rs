mod linux;
mod macos;
mod macos_network;
mod windows;

use crate::{PlatformError, PlatformResult};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Native proxy-settings backend to plan or execute.
pub enum ProxyPlatform {
    /// macOS network services managed through `networksetup`.
    Macos,
    /// Current-user Windows Internet Settings registry values.
    Windows,
    /// Linux desktop settings managed through `gsettings` and environment guidance.
    Linux,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// HTTP proxy endpoint written to native operating-system settings.
pub struct ProxyTarget {
    /// Hostname or IP address stored without a URL scheme.
    pub host: String,
    /// TCP listener port.
    pub port: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Optional target and platform-specific scope for a system-proxy operation.
pub struct ProxyOptions {
    /// Proxy endpoint required by every mutating operation and included in change reports.
    pub target: Option<ProxyTarget>,
    /// Hosts or domains that should bypass the configured proxy.
    pub bypass: Option<Vec<String>>,
    /// Single macOS network service to mutate, when selected explicitly.
    pub service: Option<String>,
    /// Whether macOS mutations should cover every enabled network service.
    pub all_services: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Read-only or mutating system-proxy operation.
pub enum ProxyAction {
    /// Inspect native settings without changing them.
    Status,
    /// Configure and enable HTTP and HTTPS proxy routing.
    Enable,
    /// Disable proxy routing while retaining a target in the change report.
    Disable,
}

impl ProxyAction {
    const fn enabled(self) -> Option<bool> {
        match self {
            Self::Status => None,
            Self::Enable => Some(true),
            Self::Disable => Some(false),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Result of inspecting or mutating native system-proxy state.
pub enum ProxyOutcome {
    /// Platform-specific settings observed without mutation.
    Status(ProxyStatus),
    /// Logical changes applied to the selected service or platform.
    Changed(Vec<ProxyChange>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Reviewable sequence of external commands and logical changes for an operation.
pub struct ProxyPlan {
    /// Backend for which the plan is valid.
    pub platform: ProxyPlatform,
    /// Ordered commands and resulting state changes.
    pub steps: Vec<ProxyPlanStep>,
}

impl ProxyPlan {
    pub(super) fn new(platform: ProxyPlatform, steps: Vec<ProxyPlanStep>) -> Self {
        Self { platform, steps }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One ordered element in a system-proxy dry-run plan.
pub enum ProxyPlanStep {
    /// External platform command that would be executed.
    Command(ProxyCommand),
    /// Logical state change that the command sequence is expected to produce.
    Change(ProxyChange),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Sanitized external command used by a native proxy backend.
pub enum ProxyCommand {
    /// Invocation of the macOS `networksetup` utility.
    MacosNetworkSetup {
        /// Exact arguments passed after the executable name.
        args: Vec<String>,
    },
    /// Invocation of the Windows `reg` utility for current-user Internet Settings.
    WindowsRegistry {
        /// Exact arguments passed after the executable name.
        args: Vec<String>,
    },
    /// Invocation of Linux desktop `gsettings`.
    LinuxGsettings {
        /// Exact arguments passed after the executable name.
        args: Vec<String>,
    },
    /// Shell-independent description of Linux proxy environment assignments.
    LinuxEnvironment {
        /// Environment assignment tokens proposed for user-managed process startup.
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Normalized logical proxy mutation independent of the platform command syntax.
pub struct ProxyChange {
    /// Backend that received the mutation.
    pub platform: ProxyPlatform,
    /// Whether proxy routing is enabled after the change.
    pub enabled: bool,
    /// Endpoint associated with the mutation.
    pub target: ProxyTarget,
    /// Bypass list written with the change, when one was supplied.
    pub bypass: Option<Vec<String>>,
    /// macOS network service affected by the change, if applicable.
    pub service: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Native settings returned by a read-only status operation.
pub enum ProxyStatus {
    /// Per-network-service state reported by macOS.
    Macos {
        /// Selected services and their HTTP, HTTPS, and bypass settings.
        services: Vec<MacosServiceStatus>,
    },
    /// Current-user Internet Settings reported by Windows.
    Windows {
        /// Whether the native proxy-enable flag is set.
        enabled: bool,
        /// Raw proxy-server value, when configured.
        server: Option<String>,
        /// Raw bypass-list value, when configured.
        bypass: Option<String>,
    },
    /// Relevant GNOME settings reported by a Linux desktop.
    Linux {
        /// Schema/key/value triples in deterministic query order.
        settings: Vec<LinuxSettingStatus>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One Linux desktop setting returned by `gsettings get`.
pub struct LinuxSettingStatus {
    /// GSettings schema queried.
    pub schema: String,
    /// Key queried within the schema.
    pub key: String,
    /// Raw textual value emitted by `gsettings`.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// HTTP, HTTPS, and bypass state for one macOS network service.
pub struct MacosServiceStatus {
    /// Human-readable network service name accepted by `networksetup`.
    pub service: String,
    /// Web proxy state reported for HTTP.
    pub http: MacosEndpointStatus,
    /// Secure web proxy state reported for HTTPS.
    pub https: MacosEndpointStatus,
    /// Bypass domains or the sanitized error returned by the bypass query.
    pub bypass: MacosBypassStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Parsed endpoint state from one macOS proxy query.
pub struct MacosEndpointStatus {
    /// Parsed `Enabled` state, defaulting to false when the field is absent.
    pub enabled: bool,
    /// Configured proxy server, when reported.
    pub server: Option<String>,
    /// Parsed proxy port, when numeric and reported.
    pub port: Option<u16>,
    /// Parsed authentication-required state.
    pub authenticated: bool,
    /// Raw `Enabled` value retained for diagnostics.
    pub reported_enabled: Option<String>,
    /// Raw `Port` value retained for diagnostics whether or not parsing succeeds.
    pub reported_port: Option<String>,
    /// Raw `Authenticated Proxy Enabled` value retained for diagnostics.
    pub reported_authenticated: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Result of querying bypass domains for a macOS network service.
pub enum MacosBypassStatus {
    /// Bypass domains in the order reported by `networksetup`.
    Domains(Vec<String>),
    /// Sanitized query failure; endpoint status remains usable.
    QueryError(String),
}

/// Builds a deterministic dry-run plan without invoking platform commands that mutate settings.
///
/// Read-only discovery may still be required to select macOS network services. Invalid option
/// combinations and missing mutation targets are returned as [`crate::PlatformError`].
pub fn plan_system_proxy(
    platform: ProxyPlatform,
    action: ProxyAction,
    options: &ProxyOptions,
) -> PlatformResult<ProxyPlan> {
    match platform {
        ProxyPlatform::Macos => macos::plan(action, options),
        ProxyPlatform::Windows => windows::plan(action, options),
        ProxyPlatform::Linux => linux::plan(action, options),
    }
}

/// Inspects or mutates native proxy settings for the selected backend.
///
/// Mutations can require existing user or administrator permissions. Commands have bounded
/// execution time and this function does not attempt privilege escalation.
pub fn execute_system_proxy(
    platform: ProxyPlatform,
    action: ProxyAction,
    options: &ProxyOptions,
) -> PlatformResult<ProxyOutcome> {
    match platform {
        ProxyPlatform::Macos => macos::execute(action, options),
        ProxyPlatform::Windows => windows::execute(action, options),
        ProxyPlatform::Linux => linux::execute(action, options),
    }
}

fn required_target(options: &ProxyOptions) -> PlatformResult<&ProxyTarget> {
    options.target.as_ref().ok_or_else(|| {
        PlatformError::InvalidState("system proxy mutation requires a proxy target".to_string())
    })
}

fn display_command_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() {
                "\"\"".to_string()
            } else if arg.chars().any(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_command_output(program: &str, args: &[String]) -> PlatformResult<std::process::Output> {
    let label = format!("{program} {}", display_command_args(args));
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| PlatformError::Io {
            context: label.clone(),
            source,
        })?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child
            .try_wait()
            .map_err(|source| PlatformError::Io {
                context: label.clone(),
                source,
            })?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|source| PlatformError::Io {
                    context: label.clone(),
                    source,
                })?;
            if output.status.success() {
                return Ok(output);
            }
            return Err(PlatformError::CommandFailed {
                command: label,
                status: output.status.code(),
                output: platform_output_message_prefer_stderr(&output),
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|source| PlatformError::Io {
                    context: label.clone(),
                    source,
                })?;
            return Err(PlatformError::Timeout {
                operation: label,
                timeout_ms: 15_000,
                output: platform_output_message_prefer_stderr(&output),
            });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(target_os = "macos")]
fn command_output(
    label: &str,
    program: &str,
    args: &[String],
    timeout: Duration,
    timeout_hint: Option<&str>,
) -> PlatformResult<std::process::Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| PlatformError::Io {
            context: label.to_string(),
            source,
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|source| PlatformError::Io {
                context: label.to_string(),
                source,
            })?
            .is_some()
        {
            return child
                .wait_with_output()
                .map_err(|source| PlatformError::Io {
                    context: label.to_string(),
                    source,
                });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|source| PlatformError::Io {
                    context: label.to_string(),
                    source,
                })?;
            let output = timeout_hint.map_or_else(
                || platform_output_message(&output),
                |hint| format!("{hint}: {}", platform_output_message(&output)),
            );
            return Err(PlatformError::Timeout {
                operation: label.to_string(),
                timeout_ms: timeout.as_millis().try_into().unwrap_or(u64::MAX),
                output,
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_output_message_prefer_stderr(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        output.status.to_string()
    }
}

#[cfg(target_os = "macos")]
fn platform_output_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => format!("{stderr}; {stdout}"),
        (false, true) => stderr,
        (true, false) => stdout,
        (true, true) => output.status.to_string(),
    }
}

#[cfg(test)]
mod tests;
