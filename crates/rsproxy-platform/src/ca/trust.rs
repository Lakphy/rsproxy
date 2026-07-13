use super::{CaPaths, certificate_fingerprint_sha256};
use crate::{PlatformError, PlatformResult};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustAction {
    Install,
    Uninstall,
}

impl TrustAction {
    pub fn completed_name(self) -> &'static str {
        match self {
            Self::Install => "installed",
            Self::Uninstall => "uninstalled",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustOptions {
    pub keychain: Option<PathBuf>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustOutcome {
    pub platform: &'static str,
    pub backend: &'static str,
    pub action: TrustAction,
    pub certificate: PathBuf,
    pub fingerprint_sha256: String,
    pub keychain: Option<PathBuf>,
    pub thumbprint_sha1: Option<String>,
    pub dry_run: bool,
    pub commands: Vec<TrustCommand>,
    pub trust_settings_removed: Option<bool>,
    pub removed_certificate: Option<bool>,
    pub installed: Option<bool>,
}

struct RootCaInfo {
    certificate_path: PathBuf,
    #[cfg(target_os = "windows")]
    certificate_pem: String,
    fingerprint_sha256: String,
}

pub fn install_root_ca(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    install_root_ca_impl(ca_directory, options)
}

pub fn uninstall_root_ca(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    uninstall_root_ca_impl(ca_directory, options)
}

fn root_ca_info(ca_directory: &Path) -> PlatformResult<RootCaInfo> {
    let certificate_path = CaPaths::new(ca_directory).certificate;
    let certificate_pem =
        fs::read_to_string(&certificate_path).map_err(|source| PlatformError::Io {
            context: format!("read {}", certificate_path.display()),
            source,
        })?;
    let fingerprint_sha256 = certificate_fingerprint_sha256(&certificate_pem).ok_or_else(|| {
        PlatformError::InvalidState(format!(
            "invalid certificate {}",
            certificate_path.display()
        ))
    })?;
    Ok(RootCaInfo {
        certificate_path,
        #[cfg(target_os = "windows")]
        certificate_pem,
        fingerprint_sha256,
    })
}

#[cfg(target_os = "macos")]
fn install_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    macos::install(ca_directory, options)
}

#[cfg(target_os = "linux")]
fn install_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    linux::install(ca_directory, options)
}

#[cfg(target_os = "windows")]
fn install_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    windows::install(ca_directory, options)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn install_root_ca_impl(
    _ca_directory: &Path,
    _options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    Err(PlatformError::Unsupported(
        "CA install is unsupported on this platform".to_string(),
    ))
}

#[cfg(target_os = "macos")]
fn uninstall_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    macos::uninstall(ca_directory, options)
}

#[cfg(target_os = "linux")]
fn uninstall_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    linux::uninstall(ca_directory, options)
}

#[cfg(target_os = "windows")]
fn uninstall_root_ca_impl(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    windows::uninstall(ca_directory, options)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn uninstall_root_ca_impl(
    _ca_directory: &Path,
    _options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    Err(PlatformError::Unsupported(
        "CA uninstall is unsupported on this platform".to_string(),
    ))
}

#[cfg(target_os = "macos")]
pub fn keychain_contains_fingerprint(keychain: &Path, fingerprint: &str) -> PlatformResult<bool> {
    macos::keychain_contains_fingerprint(keychain, fingerprint)
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_contains_fingerprint(_keychain: &Path, _fingerprint: &str) -> PlatformResult<bool> {
    Err(PlatformError::Unsupported(
        "CA keychain status is only implemented for macOS security keychains in this build"
            .to_string(),
    ))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_command_output(program: &str, args: &[String]) -> PlatformResult<()> {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

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
                return Ok(());
            }
            return Err(PlatformError::CommandFailed {
                command: label,
                status: output.status.code(),
                output: platform_output_message(&output),
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
                output: platform_output_message(&output),
            });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
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
fn platform_output_message(output: &std::process::Output) -> String {
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
