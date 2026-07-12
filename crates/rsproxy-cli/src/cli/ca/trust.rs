use super::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

pub(in crate::cli) fn ca_keychain_arg(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(idx) = args.iter().position(|arg| arg == "--keychain") else {
        return Ok(None);
    };
    let value = args
        .get(idx + 1)
        .ok_or_else(|| "--keychain requires a path".to_string())?;
    if value.starts_with('-') {
        return Err("--keychain requires a path".to_string());
    }
    Ok(Some(PathBuf::from(value)))
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_install(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, _cert, fingerprint) = ca_cert_info(ca_dir)?;
    let keychain = ca_target_keychain(args)?;
    if has_flag(args, "--dry-run") {
        let command = vec![
            "add-trusted-cert".to_string(),
            "-r".to_string(),
            "trustRoot".to_string(),
            "-p".to_string(),
            "ssl".to_string(),
            "-k".to_string(),
            keychain.display().to_string(),
            cert_path.display().to_string(),
        ];
        print_ca_trust_plan("macos", "security", &command, args);
        return Ok(());
    }
    if !keychain.is_file() {
        return Err(format!("keychain not found: {}", keychain.display()));
    }

    let mut cmd = Command::new("security");
    cmd.arg("add-trusted-cert")
        .arg("-r")
        .arg("trustRoot")
        .arg("-p")
        .arg("ssl")
        .arg("-k")
        .arg(&keychain)
        .arg(&cert_path);
    security_output("security add-trusted-cert", &mut cmd)?;

    println!(
        "installed cert={} keychain={} fingerprint_sha256={}",
        cert_path.display(),
        keychain.display(),
        fingerprint
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub(in crate::cli) fn ca_install(args: &[String], ca_dir: &Path) -> Result<(), String> {
    linux::install(args, ca_dir)
}

#[cfg(target_os = "windows")]
pub(in crate::cli) fn ca_install(args: &[String], ca_dir: &Path) -> Result<(), String> {
    windows::install(args, ca_dir)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(in crate::cli) fn ca_install(_args: &[String], _ca_dir: &Path) -> Result<(), String> {
    Err("CA install is unsupported on this platform".to_string())
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_uninstall(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, _cert, fingerprint) = ca_cert_info(ca_dir)?;
    let keychain = ca_target_keychain(args)?;
    if has_flag(args, "--dry-run") {
        let commands = [
            vec![
                "remove-trusted-cert".to_string(),
                cert_path.display().to_string(),
            ],
            vec![
                "delete-certificate".to_string(),
                "-Z".to_string(),
                compact_fingerprint(&fingerprint),
                "-t".to_string(),
                keychain.display().to_string(),
            ],
        ];
        for command in commands {
            print_ca_trust_plan("macos", "security", &command, args);
        }
        return Ok(());
    }
    if !keychain.is_file() {
        return Err(format!("keychain not found: {}", keychain.display()));
    }

    let trust_settings_removed = if ca_trust_settings_contains_fingerprint(&fingerprint)? {
        ca_remove_trusted_cert(&cert_path)?;
        true
    } else {
        false
    };
    let removed_certificate = ca_delete_keychain_cert(&keychain, &fingerprint)?;
    let installed = ca_keychain_contains_fingerprint(&keychain, &fingerprint)?;

    println!(
        "uninstalled cert={} keychain={} fingerprint_sha256={} trust_settings_removed={} removed_certificate={} installed={}",
        cert_path.display(),
        keychain.display(),
        fingerprint,
        trust_settings_removed,
        removed_certificate,
        installed
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub(in crate::cli) fn ca_uninstall(args: &[String], ca_dir: &Path) -> Result<(), String> {
    linux::uninstall(args, ca_dir)
}

#[cfg(target_os = "windows")]
pub(in crate::cli) fn ca_uninstall(args: &[String], ca_dir: &Path) -> Result<(), String> {
    windows::uninstall(args, ca_dir)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(in crate::cli) fn ca_uninstall(_args: &[String], _ca_dir: &Path) -> Result<(), String> {
    Err("CA uninstall is unsupported on this platform".to_string())
}

pub(in crate::cli) fn print_ca_trust_plan(
    platform: &str,
    program: &str,
    command: &[String],
    args: &[String],
) {
    let rendered = format!(
        "dry-run {platform} {program} {}",
        display_command_args(command)
    );
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": platform,
                "dry_run": true,
                "program": program,
                "args": command,
            })
        );
    } else {
        println!("{rendered}");
    }
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_target_keychain(args: &[String]) -> Result<PathBuf, String> {
    if let Some(keychain) = ca_keychain_arg(args)? {
        return Ok(keychain);
    }
    macos_login_keychain()
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn macos_login_keychain() -> Result<PathBuf, String> {
    let mut cmd = Command::new("security");
    cmd.arg("login-keychain");
    let output = security_output("security login-keychain", &mut cmd)?;
    let path = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('"')
        .to_string();
    if path.is_empty() {
        return Err("security login-keychain returned an empty path".to_string());
    }
    Ok(PathBuf::from(path))
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_keychain_contains_fingerprint(
    keychain: &Path,
    fingerprint: &str,
) -> Result<bool, String> {
    if !keychain.is_file() {
        return Err(format!("keychain not found: {}", keychain.display()));
    }
    let compact = compact_fingerprint(fingerprint);
    let mut cmd = Command::new("security");
    cmd.arg("find-certificate")
        .arg("-a")
        .arg("-Z")
        .arg(keychain);
    let output = security_raw_output("security find-certificate", &mut cmd)?;
    if !output.status.success() {
        if security_output_is_not_found(&output) {
            return Ok(false);
        }
        return Err(format!(
            "security find-certificate failed: {}",
            security_output_message(&output)
        ));
    }
    let stdout = compact_fingerprint(&String::from_utf8_lossy(&output.stdout));
    Ok(stdout.contains(&compact))
}

#[cfg(not(target_os = "macos"))]
pub(in crate::cli) fn ca_keychain_contains_fingerprint(
    _keychain: &Path,
    _fingerprint: &str,
) -> Result<bool, String> {
    Err(
        "CA keychain status is only implemented for macOS security keychains in this build"
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_remove_trusted_cert(cert_path: &Path) -> Result<(), String> {
    let mut cmd = Command::new("security");
    cmd.arg("remove-trusted-cert").arg(cert_path);
    let output = security_raw_output("security remove-trusted-cert", &mut cmd)?;
    if output.status.success() || security_output_is_not_found(&output) {
        return Ok(());
    }
    Err(format!(
        "security remove-trusted-cert failed: {}",
        security_output_message(&output)
    ))
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_delete_keychain_cert(
    keychain: &Path,
    fingerprint: &str,
) -> Result<bool, String> {
    let compact = compact_fingerprint(fingerprint);
    let mut cmd = Command::new("security");
    cmd.arg("delete-certificate")
        .arg("-Z")
        .arg(&compact)
        .arg("-t")
        .arg(keychain);
    let output = security_raw_output("security delete-certificate", &mut cmd)?;
    if output.status.success() {
        return Ok(true);
    }
    if security_output_is_not_found(&output) {
        return Ok(false);
    }
    Err(format!(
        "security delete-certificate failed: {}",
        security_output_message(&output)
    ))
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn ca_trust_settings_contains_fingerprint(
    fingerprint: &str,
) -> Result<bool, String> {
    let compact = compact_fingerprint(fingerprint);
    let mut cmd = Command::new("security");
    cmd.arg("dump-trust-settings");
    let output = security_raw_output("security dump-trust-settings", &mut cmd)?;
    if !output.status.success() {
        if security_output_is_not_found(&output) {
            return Ok(false);
        }
        return Err(format!(
            "security dump-trust-settings failed: {}",
            security_output_message(&output)
        ));
    }
    let stdout = compact_fingerprint(&String::from_utf8_lossy(&output.stdout));
    Ok(stdout.contains(&compact))
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn security_output(
    label: &str,
    cmd: &mut Command,
) -> Result<std::process::Output, String> {
    let output = security_raw_output(label, cmd)?;
    if !output.status.success() {
        return Err(format!(
            "{label} failed: {}",
            security_output_message(&output)
        ));
    }
    Ok(output)
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn security_raw_output(
    label: &str,
    cmd: &mut Command,
) -> Result<std::process::Output, String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{label}: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child
            .try_wait()
            .map_err(|e| format!("{label}: {e}"))?
            .is_some()
        {
            return child
                .wait_with_output()
                .map_err(|e| format!("{label}: {e}"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|e| format!("{label}: {e}"))?;
            return Err(format!(
                "{label} timed out after 15s; macOS may be waiting for an authentication dialog: {}",
                security_output_message(&output)
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn security_output_is_not_found(output: &std::process::Output) -> bool {
    let text = security_output_message(output).to_ascii_lowercase();
    text.contains("could not be found")
        || text.contains("not found")
        || text.contains("no matching")
        || text.contains("unable to find")
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn security_output_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => format!("{stderr}; {stdout}"),
        (false, true) => stderr,
        (true, false) => stdout,
        (true, true) => output.status.to_string(),
    }
}
