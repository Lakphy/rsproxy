use super::*;
use crate::ca::certificates::certificate_fingerprint_sha1;

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
    let thumbprint_sha1 = certificate_fingerprint_sha1(&info.certificate_pem).ok_or_else(|| {
        PlatformError::InvalidState(format!(
            "invalid certificate {}",
            info.certificate_path.display()
        ))
    })?;
    let args = match action {
        TrustAction::Install => vec![
            "-user".to_string(),
            "-addstore".to_string(),
            "Root".to_string(),
            info.certificate_path.display().to_string(),
        ],
        TrustAction::Uninstall => vec![
            "-user".to_string(),
            "-delstore".to_string(),
            "Root".to_string(),
            thumbprint_sha1.clone(),
        ],
    };
    let command = TrustCommand {
        program: "certutil".to_string(),
        args,
    };
    if !options.dry_run {
        platform_command_output(&command.program, &command.args)?;
    }
    Ok(TrustOutcome {
        platform: "windows",
        backend: "current-user-root-store",
        action,
        certificate: info.certificate_path,
        fingerprint_sha256: info.fingerprint_sha256,
        keychain: None,
        thumbprint_sha1: Some(thumbprint_sha1),
        dry_run: options.dry_run,
        commands: vec![command],
        trust_settings_removed: None,
        removed_certificate: None,
        installed: None,
    })
}
