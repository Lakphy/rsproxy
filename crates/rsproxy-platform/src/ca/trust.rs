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
/// Requested mutation of the host certificate trust store.
pub enum TrustAction {
    /// Add the rsproxy root certificate as trusted.
    Install,
    /// Remove trust and, where supported, the matching certificate.
    Uninstall,
}

impl TrustAction {
    /// Returns the stable past-tense label used in command results.
    pub fn completed_name(self) -> &'static str {
        match self {
            Self::Install => "installed",
            Self::Uninstall => "uninstalled",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// Platform-specific trust-store selection and execution policy.
pub struct TrustOptions {
    /// Explicit macOS keychain; unsupported platforms reject or ignore it as documented by their backend.
    pub keychain: Option<PathBuf>,
    /// When true, validate inputs and return planned commands without executing them.
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Sanitized external command included in a trust-operation report.
pub struct TrustCommand {
    /// Executable name without shell interpolation.
    pub program: String,
    /// Exact argument vector passed or planned for the executable.
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Auditable result of a trust-store install or uninstall operation.
pub struct TrustOutcome {
    /// Stable operating-system name selected at compile time.
    pub platform: &'static str,
    /// Platform trust-store backend, such as a keychain or certificate database.
    pub backend: &'static str,
    /// Requested install or uninstall mutation.
    pub action: TrustAction,
    /// Root certificate path used by the backend.
    pub certificate: PathBuf,
    /// Uppercase, colon-delimited SHA-256 fingerprint of that certificate.
    pub fingerprint_sha256: String,
    /// macOS keychain targeted by the operation, when applicable.
    pub keychain: Option<PathBuf>,
    /// Windows SHA-1 thumbprint used for certificate-store lookup, when applicable.
    pub thumbprint_sha1: Option<String>,
    /// Whether commands were planned but deliberately not executed.
    pub dry_run: bool,
    /// External commands executed or proposed, in operation order.
    pub commands: Vec<TrustCommand>,
    /// Whether macOS trust settings were removed; absent on other backends or dry runs.
    pub trust_settings_removed: Option<bool>,
    /// Whether a matching certificate object was removed; absent when not observable.
    pub removed_certificate: Option<bool>,
    /// Whether installation was confirmed; absent when the backend cannot confirm or on dry runs.
    pub installed: Option<bool>,
}

struct RootCaInfo {
    certificate_path: PathBuf,
    #[cfg(target_os = "windows")]
    certificate_pem: String,
    fingerprint_sha256: String,
}

/// Installs the persisted rsproxy root into the native trust store.
///
/// The root certificate must already exist and parse correctly. This may invoke privileged host
/// commands; the function never attempts to elevate privileges itself.
pub fn install_root_ca(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    install_root_ca_impl(ca_directory, options)
}

/// Removes the persisted rsproxy root from the native trust store by certificate identity.
///
/// This may invoke privileged host commands; the function never attempts to elevate privileges.
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
/// Checks whether a macOS keychain contains a certificate with `fingerprint`.
///
/// The comparison ignores fingerprint colons and ASCII case.
pub fn keychain_contains_fingerprint(keychain: &Path, fingerprint: &str) -> PlatformResult<bool> {
    macos::keychain_contains_fingerprint(keychain, fingerprint)
}

#[cfg(not(target_os = "macos"))]
/// Reports that direct keychain fingerprint lookup is unavailable outside macOS builds.
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
