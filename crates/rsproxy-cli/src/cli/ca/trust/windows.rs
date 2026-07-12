use super::*;

pub(super) fn install(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, cert, fingerprint) = ca_cert_info(ca_dir)?;
    let command = vec![
        "-user".to_string(),
        "-addstore".to_string(),
        "Root".to_string(),
        cert_path.display().to_string(),
    ];
    if has_flag(args, "--dry-run") {
        print_ca_trust_plan("windows", "certutil", &command, args);
        return Ok(());
    }
    platform_command_output("certutil", &command)?;
    print_ca_result(args, "installed", &cert_path, &fingerprint, &cert)?;
    Ok(())
}

pub(super) fn uninstall(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, cert, fingerprint) = ca_cert_info(ca_dir)?;
    let sha1 = cert_sha1_fingerprint(&cert)
        .ok_or_else(|| format!("invalid certificate {}", cert_path.display()))?;
    let command = vec![
        "-user".to_string(),
        "-delstore".to_string(),
        "Root".to_string(),
        sha1.clone(),
    ];
    if has_flag(args, "--dry-run") {
        print_ca_trust_plan("windows", "certutil", &command, args);
        return Ok(());
    }
    platform_command_output("certutil", &command)?;
    print_ca_result(args, "uninstalled", &cert_path, &fingerprint, &cert)?;
    Ok(())
}

fn print_ca_result(
    args: &[String],
    action: &str,
    cert_path: &Path,
    fingerprint: &str,
    cert: &str,
) -> Result<(), String> {
    let sha1 = cert_sha1_fingerprint(cert)
        .ok_or_else(|| format!("invalid certificate {}", cert_path.display()))?;
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "windows",
                "backend": "current-user-root-store",
                "action": action,
                "cert": cert_path.display().to_string(),
                "fingerprint_sha256": fingerprint,
                "thumbprint_sha1": sha1,
            })
        );
    } else {
        println!(
            "{action} cert={} fingerprint_sha256={fingerprint} thumbprint_sha1={sha1}",
            cert_path.display()
        );
    }
    Ok(())
}
