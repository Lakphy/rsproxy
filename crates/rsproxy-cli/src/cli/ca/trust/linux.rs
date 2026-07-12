use super::*;

pub(super) fn install(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, _cert, fingerprint) = ca_cert_info(ca_dir)?;
    let command = vec![
        "anchor".to_string(),
        "--store".to_string(),
        cert_path.display().to_string(),
    ];
    if has_flag(args, "--dry-run") {
        print_ca_trust_plan("linux", "trust", &command, args);
        return Ok(());
    }
    platform_command_output("trust", &command)?;
    print_ca_result(args, "installed", &cert_path, &fingerprint);
    Ok(())
}

pub(super) fn uninstall(args: &[String], ca_dir: &Path) -> Result<(), String> {
    let (cert_path, _cert, fingerprint) = ca_cert_info(ca_dir)?;
    let command = vec![
        "anchor".to_string(),
        "--remove".to_string(),
        cert_path.display().to_string(),
    ];
    if has_flag(args, "--dry-run") {
        print_ca_trust_plan("linux", "trust", &command, args);
        return Ok(());
    }
    platform_command_output("trust", &command)?;
    print_ca_result(args, "uninstalled", &cert_path, &fingerprint);
    Ok(())
}

fn print_ca_result(args: &[String], action: &str, cert_path: &Path, fingerprint: &str) {
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "linux",
                "backend": "p11-kit",
                "action": action,
                "cert": cert_path.display().to_string(),
                "fingerprint_sha256": fingerprint,
            })
        );
    } else {
        println!(
            "{action} cert={} fingerprint_sha256={fingerprint}",
            cert_path.display()
        );
    }
}
