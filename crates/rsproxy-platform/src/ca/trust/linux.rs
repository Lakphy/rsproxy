use super::*;

pub(super) fn install(ca_directory: &Path, options: &TrustOptions) -> PlatformResult<TrustOutcome> {
    execute(ca_directory, options, TrustAction::Install)
}

pub(super) fn uninstall(
    ca_directory: &Path,
    options: &TrustOptions,
) -> PlatformResult<TrustOutcome> {
    execute(ca_directory, options, TrustAction::Uninstall)
}

fn execute(
    ca_directory: &Path,
    options: &TrustOptions,
    action: TrustAction,
) -> PlatformResult<TrustOutcome> {
    let info = root_ca_info(ca_directory)?;
    let command = TrustCommand {
        program: "trust".to_string(),
        args: vec![
            "anchor".to_string(),
            match action {
                TrustAction::Install => "--store",
                TrustAction::Uninstall => "--remove",
            }
            .to_string(),
            info.certificate_path.display().to_string(),
        ],
    };
    if !options.dry_run {
        platform_command_output(&command.program, &command.args)?;
    }
    Ok(TrustOutcome {
        platform: "linux",
        backend: "p11-kit",
        action,
        certificate: info.certificate_path,
        fingerprint_sha256: info.fingerprint_sha256,
        keychain: None,
        thumbprint_sha1: None,
        dry_run: options.dry_run,
        commands: vec![command],
        trust_settings_removed: None,
        removed_certificate: None,
        installed: None,
    })
}
