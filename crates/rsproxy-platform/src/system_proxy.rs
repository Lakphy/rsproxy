mod linux;
mod macos;
mod macos_network;
mod windows;

use crate::{PlatformError, PlatformResult};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyPlatform {
    Macos,
    Windows,
    Linux,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyTarget {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyOptions {
    pub target: Option<ProxyTarget>,
    pub bypass: Option<Vec<String>>,
    pub service: Option<String>,
    pub all_services: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyAction {
    Status,
    Enable,
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
pub enum ProxyOutcome {
    Status(ProxyStatus),
    Changed(Vec<ProxyChange>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyPlan {
    pub platform: ProxyPlatform,
    pub steps: Vec<ProxyPlanStep>,
}

impl ProxyPlan {
    pub(super) fn new(platform: ProxyPlatform, steps: Vec<ProxyPlanStep>) -> Self {
        Self { platform, steps }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyPlanStep {
    Command(ProxyCommand),
    Change(ProxyChange),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyCommand {
    MacosNetworkSetup { args: Vec<String> },
    WindowsRegistry { args: Vec<String> },
    LinuxGsettings { args: Vec<String> },
    LinuxEnvironment { args: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyChange {
    pub platform: ProxyPlatform,
    pub enabled: bool,
    pub target: ProxyTarget,
    pub bypass: Option<Vec<String>>,
    pub service: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyStatus {
    Macos {
        services: Vec<MacosServiceStatus>,
    },
    Windows {
        enabled: bool,
        server: Option<String>,
        bypass: Option<String>,
    },
    Linux {
        settings: Vec<LinuxSettingStatus>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxSettingStatus {
    pub schema: String,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacosServiceStatus {
    pub service: String,
    pub http: MacosEndpointStatus,
    pub https: MacosEndpointStatus,
    pub bypass: MacosBypassStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacosEndpointStatus {
    pub enabled: bool,
    pub server: Option<String>,
    pub port: Option<u16>,
    pub authenticated: bool,
    pub reported_enabled: Option<String>,
    pub reported_port: Option<String>,
    pub reported_authenticated: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacosBypassStatus {
    Domains(Vec<String>),
    QueryError(String),
}

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
