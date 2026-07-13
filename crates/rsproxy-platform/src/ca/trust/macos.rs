use super::*;
use crate::ca::certificates::compact_fingerprint;
use std::process::Command;

pub(super) fn install(ca_directory: &Path, options: &TrustOptions) -> PlatformResult<TrustOutcome> {
    let info = root_ca_info(ca_directory)?;
    let keychain = target_keychain(options)?;
    let command = TrustCommand {
        program: "security".to_string(),
        args: vec![
            "add-trusted-cert".to_string(),
            "-r".to_string(),
            "trustRoot".to_string(),
            "-p".to_string(),
            "ssl".to_string(),
            "-k".to_string(),
            keychain.display().to_string(),
            info.certificate_path.display().to_string(),
        ],
    };
    if !options.dry_run {
        if !keychain.is_file() {
            return Err(PlatformError::InvalidState(format!(
                "keychain not found: {}",
                keychain.display()
            )));
        }
        let mut process = Command::new(&command.program);
        process.args(&command.args);
        security_output("security add-trusted-cert", &mut process)?;
    }
    Ok(TrustOutcome {
        platform: "macos",
        backend: "security-keychain",
        action: TrustAction::Install,
        certificate: info.certificate_path,
        fingerprint_sha256: info.fingerprint_sha256,
        keychain: Some(keychain),
        thumbprint_sha1: None,
        dry_run: options.dry_run,
        commands: vec![command],
        trust_settings_removed: None,
        removed_certificate: None,
        installed: None,
    })
}

pub(super) fn uninstall(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    let info = root_ca_info(ca_directory)?;
    let keychain = target_keychain(options)?;
    let commands = vec![
        TrustCommand {
            program: "security".to_string(),
            args: vec![
                "remove-trusted-cert".to_string(),
                info.certificate_path.display().to_string(),
            ],
        },
        TrustCommand {
            program: "security".to_string(),
            args: vec![
                "delete-certificate".to_string(),
                "-Z".to_string(),
                compact_fingerprint(&info.fingerprint_sha256),
                "-t".to_string(),
                keychain.display().to_string(),
            ],
        },
    ];
    if options.dry_run {
        return Ok(TrustOutcome {
            platform: "macos",
            backend: "security-keychain",
            action: TrustAction::Uninstall,
            certificate: info.certificate_path,
            fingerprint_sha256: info.fingerprint_sha256,
            keychain: Some(keychain),
            thumbprint_sha1: None,
            dry_run: true,
            commands,
            trust_settings_removed: None,
            removed_certificate: None,
            installed: None,
        });
    }
    if !keychain.is_file() {
        return Err(PlatformError::InvalidState(format!(
            "keychain not found: {}",
            keychain.display()
        )));
    }

    let trust_settings_removed = if trust_settings_contains_fingerprint(&info.fingerprint_sha256)? {
        remove_trusted_certificate(&info.certificate_path)?;
        true
    } else {
        false
    };
    let removed_certificate = delete_keychain_certificate(&keychain, &info.fingerprint_sha256)?;
    let installed = keychain_contains_fingerprint(&keychain, &info.fingerprint_sha256)?;

    Ok(TrustOutcome {
        platform: "macos",
        backend: "security-keychain",
        action: TrustAction::Uninstall,
        certificate: info.certificate_path,
        fingerprint_sha256: info.fingerprint_sha256,
        keychain: Some(keychain),
        thumbprint_sha1: None,
        dry_run: false,
        commands,
        trust_settings_removed: Some(trust_settings_removed),
        removed_certificate: Some(removed_certificate),
        installed: Some(installed),
    })
}

pub(super) fn keychain_contains_fingerprint(
    keychain: &Path,
    fingerprint: &str,
) -> PlatformResult<bool> {
    if !keychain.is_file() {
        return Err(PlatformError::InvalidState(format!(
            "keychain not found: {}",
            keychain.display()
        )));
    }
    let compact = compact_fingerprint(fingerprint);
    let mut command = Command::new("security");
    command
        .arg("find-certificate")
        .arg("-a")
        .arg("-Z")
        .arg(keychain);
    let output = security_raw_output("security find-certificate", &mut command)?;
    if !output.status.success() {
        if security_output_is_not_found(&output) {
            return Ok(false);
        }
        return Err(PlatformError::CommandFailed {
            command: "security find-certificate".to_string(),
            status: output.status.code(),
            output: security_output_message(&output),
        });
    }
    let stdout = compact_fingerprint(&String::from_utf8_lossy(&output.stdout));
    Ok(stdout.contains(&compact))
}

fn target_keychain(options: &TrustOptions) -> PlatformResult<PathBuf> {
    if let Some(keychain) = &options.keychain {
        return Ok(keychain.clone());
    }
    macos_login_keychain()
}

fn macos_login_keychain() -> PlatformResult<PathBuf> {
    let mut command = Command::new("security");
    command.arg("login-keychain");
    let output = security_output("security login-keychain", &mut command)?;
    let path = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('"')
        .to_string();
    if path.is_empty() {
        return Err(PlatformError::InvalidState(
            "security login-keychain returned an empty path".to_string(),
        ));
    }
    Ok(PathBuf::from(path))
}

fn remove_trusted_certificate(certificate_path: &Path) -> PlatformResult<()> {
    let mut command = Command::new("security");
    command.arg("remove-trusted-cert").arg(certificate_path);
    let output = security_raw_output("security remove-trusted-cert", &mut command)?;
    if output.status.success() || security_output_is_not_found(&output) {
        return Ok(());
    }
    Err(PlatformError::CommandFailed {
        command: "security remove-trusted-cert".to_string(),
        status: output.status.code(),
        output: security_output_message(&output),
    })
}

fn delete_keychain_certificate(keychain: &Path, fingerprint: &str) -> PlatformResult<bool> {
    let mut command = Command::new("security");
    command
        .arg("delete-certificate")
        .arg("-Z")
        .arg(compact_fingerprint(fingerprint))
        .arg("-t")
        .arg(keychain);
    let output = security_raw_output("security delete-certificate", &mut command)?;
    if output.status.success() {
        return Ok(true);
    }
    if security_output_is_not_found(&output) {
        return Ok(false);
    }
    Err(PlatformError::CommandFailed {
        command: "security delete-certificate".to_string(),
        status: output.status.code(),
        output: security_output_message(&output),
    })
}

fn trust_settings_contains_fingerprint(fingerprint: &str) -> PlatformResult<bool> {
    let compact = compact_fingerprint(fingerprint);
    let mut command = Command::new("security");
    command.arg("dump-trust-settings");
    let output = security_raw_output("security dump-trust-settings", &mut command)?;
    if !output.status.success() {
        if security_output_is_not_found(&output) {
            return Ok(false);
        }
        return Err(PlatformError::CommandFailed {
            command: "security dump-trust-settings".to_string(),
            status: output.status.code(),
            output: security_output_message(&output),
        });
    }
    let stdout = compact_fingerprint(&String::from_utf8_lossy(&output.stdout));
    Ok(stdout.contains(&compact))
}

fn security_output(label: &str, command: &mut Command) -> PlatformResult<std::process::Output> {
    let output = security_raw_output(label, command)?;
    if !output.status.success() {
        return Err(PlatformError::CommandFailed {
            command: label.to_string(),
            status: output.status.code(),
            output: security_output_message(&output),
        });
    }
    Ok(output)
}

fn security_raw_output(label: &str, command: &mut Command) -> PlatformResult<std::process::Output> {
    use std::process::Stdio;
    use std::thread;
    use std::time::{Duration, Instant};

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| PlatformError::Io {
        context: label.to_string(),
        source,
    })?;
    let deadline = Instant::now() + Duration::from_secs(15);
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
            return Err(PlatformError::Timeout {
                operation: label.to_string(),
                timeout_ms: 15_000,
                output: format!(
                    "macOS may be waiting for an authentication dialog: {}",
                    security_output_message(&output)
                ),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn security_output_is_not_found(output: &std::process::Output) -> bool {
    let text = security_output_message(output).to_ascii_lowercase();
    text.contains("could not be found")
        || text.contains("not found")
        || text.contains("no matching")
        || text.contains("unable to find")
}

fn security_output_message(output: &std::process::Output) -> String {
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
#[path = "macos/tests.rs"]
mod tests;
