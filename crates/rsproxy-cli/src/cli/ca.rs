use super::command::{
    CaArgs, CaCommand, CaExportArgs, CaInitArgs, CaIssueArgs, CaStatusArgs, CaTrustArgs,
    RuntimeArgs,
};
use super::config::runtime_config;
use crate::{CliError, CliResult};
use rsproxy_platform::ca::{
    CaInitialization, TrustOptions, TrustOutcome, cached_leaf_certificate, initialize_root_ca,
    install_root_ca, keychain_contains_fingerprint, read_root_ca, read_root_certificate,
    root_ca_status, store_leaf_certificate, uninstall_root_ca,
};
use std::fs;
use std::path::Path;

pub(super) fn ca_cmd(args: CaArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let storage = config.engine().storage.clone();
    let ca_directory = storage.join("ca");
    match args.command {
        None => ca_status(CaStatusArgs { keychain: None }, &ca_directory, json),
        Some(CaCommand::Init(args)) => ca_init(args, &ca_directory),
        Some(CaCommand::Status(args)) => ca_status(args, &ca_directory, json),
        Some(CaCommand::Export(args)) => ca_export(args, &ca_directory),
        Some(CaCommand::Issue(args)) => ca_issue(args, &ca_directory),
        Some(CaCommand::Install(args)) => ca_install(args, &ca_directory, json),
        Some(CaCommand::Uninstall(args)) => ca_uninstall(args, &ca_directory, json),
    }
}

pub(super) fn ca_init(args: CaInitArgs, ca_directory: &Path) -> CliResult<()> {
    let common_name = args
        .name
        .unwrap_or_else(|| "rsproxy local root CA".to_string());
    match initialize_root_ca(ca_directory, &common_name, args.force)? {
        CaInitialization::AlreadyInitialized { paths } => {
            println!(
                "already initialized cert={} key={}",
                paths.certificate.display(),
                paths.private_key.display()
            );
        }
        CaInitialization::Created {
            paths,
            fingerprint_sha256,
        } => {
            println!(
                "initialized cert={} key={} fingerprint_sha256={fingerprint_sha256}",
                paths.certificate.display(),
                paths.private_key.display()
            );
        }
    }
    Ok(())
}

pub(super) fn ca_status(args: CaStatusArgs, ca_directory: &Path, json: bool) -> CliResult<()> {
    let status = root_ca_status(ca_directory)?;
    let keychain = args.keychain;
    let installed = if let Some(keychain) = &keychain {
        Some(if status.certificate_exists {
            keychain_contains_fingerprint(keychain, &status.fingerprint_sha256)?
        } else {
            false
        })
    } else {
        None
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ca_dir": status.paths.directory.display().to_string(),
                "initialized": status.initialized,
                "cert": status.certificate_exists,
                "key": status.private_key_exists,
                "leaf_cached": status.leaf_cached,
                "fingerprint_sha256": status.fingerprint_sha256,
                "cert_path": status.initialized.then(|| status.paths.certificate.display().to_string()),
                "key_path": status.initialized.then(|| status.paths.private_key.display().to_string()),
                "keychain": keychain.as_ref().map(|path| path.display().to_string()),
                "installed": installed,
            })
        );
        return Ok(());
    }
    println!(
        "ca_dir={} initialized={} cert={} key={} leaf_cached={} fingerprint_sha256={}",
        status.paths.directory.display(),
        status.initialized,
        status.certificate_exists,
        status.private_key_exists,
        status.leaf_cached,
        status.fingerprint_sha256
    );
    if status.initialized {
        println!("cert_path={}", status.paths.certificate.display());
        println!("key_path={}", status.paths.private_key.display());
    }
    if let Some(keychain) = keychain {
        println!(
            "keychain={} installed={}",
            keychain.display(),
            installed.unwrap_or(false)
        );
    }
    Ok(())
}

pub(super) fn ca_export(args: CaExportArgs, ca_directory: &Path) -> CliResult<()> {
    let certificate = read_root_certificate(ca_directory)?;
    if let Some(output) = args.output {
        fs::write(&output, certificate).map_err(|source| {
            CliError::io(
                format!("write exported CA certificate {}", output.display()),
                source,
            )
        })?;
        println!("wrote {}", output.display());
    } else {
        print!("{certificate}");
    }
    Ok(())
}

pub(super) fn ca_issue(args: CaIssueArgs, ca_directory: &Path) -> CliResult<()> {
    validate_leaf_host(&args.host)?;
    if !args.force
        && let Some(cached) = cached_leaf_certificate(ca_directory, &args.host)?
    {
        println!(
            "cached host={} cert={} key={} chain={} fingerprint_sha256={}",
            args.host,
            cached.paths.certificate.display(),
            cached.paths.private_key.display(),
            cached.paths.chain.display(),
            cached.fingerprint_sha256
        );
        return Ok(());
    }

    let root = read_root_ca(ca_directory)?;
    let issued = rsproxy_engine::issue_leaf_certificate(
        &root.certificate_pem,
        &root.private_key_pem,
        &args.host,
    )?;
    let stored = store_leaf_certificate(
        ca_directory,
        &args.host,
        &issued.certificate_pem,
        &issued.private_key_pem,
        &issued.chain_pem,
    )?;
    println!(
        "issued host={} cert={} key={} chain={} fingerprint_sha256={}",
        args.host,
        stored.paths.certificate.display(),
        stored.paths.private_key.display(),
        stored.paths.chain.display(),
        stored.fingerprint_sha256
    );
    Ok(())
}

fn ca_install(args: CaTrustArgs, ca_directory: &Path, json: bool) -> CliResult<()> {
    let outcome = install_root_ca(ca_directory, &trust_options(&args))?;
    print_trust_outcome(json, &outcome)
}

fn ca_uninstall(args: CaTrustArgs, ca_directory: &Path, json: bool) -> CliResult<()> {
    let outcome = uninstall_root_ca(ca_directory, &trust_options(&args))?;
    print_trust_outcome(json, &outcome)
}

fn trust_options(args: &CaTrustArgs) -> TrustOptions {
    TrustOptions {
        keychain: args.keychain.clone(),
        dry_run: args.dry_run,
    }
}

pub(super) fn print_trust_outcome(json: bool, outcome: &TrustOutcome) -> CliResult<()> {
    if outcome.dry_run {
        for command in &outcome.commands {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "platform": outcome.platform,
                        "dry_run": true,
                        "program": command.program,
                        "args": command.args,
                    })
                );
            } else {
                println!(
                    "dry-run {} {} {}",
                    outcome.platform,
                    command.program,
                    display_ca_command_args(&command.args)
                );
            }
        }
        return Ok(());
    }

    let action = outcome.action.completed_name();
    match outcome.platform {
        "macos" if outcome.action == rsproxy_platform::ca::TrustAction::Install => {
            let keychain = outcome
                .keychain
                .as_ref()
                .ok_or(CliError::InvalidPlatformOutcome {
                    detail: "macOS trust result is missing keychain",
                })?;
            println!(
                "{action} cert={} keychain={} fingerprint_sha256={}",
                outcome.certificate.display(),
                keychain.display(),
                outcome.fingerprint_sha256
            );
        }
        "macos" => {
            let keychain = outcome
                .keychain
                .as_ref()
                .ok_or(CliError::InvalidPlatformOutcome {
                    detail: "macOS trust result is missing keychain",
                })?;
            println!(
                "{action} cert={} keychain={} fingerprint_sha256={} trust_settings_removed={} removed_certificate={} installed={}",
                outcome.certificate.display(),
                keychain.display(),
                outcome.fingerprint_sha256,
                outcome.trust_settings_removed.unwrap_or(false),
                outcome.removed_certificate.unwrap_or(false),
                outcome.installed.unwrap_or(false)
            );
        }
        "windows" => {
            let thumbprint =
                outcome
                    .thumbprint_sha1
                    .as_deref()
                    .ok_or(CliError::InvalidPlatformOutcome {
                        detail: "Windows trust result is missing SHA-1 thumbprint",
                    })?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "platform": outcome.platform,
                        "backend": outcome.backend,
                        "action": action,
                        "cert": outcome.certificate.display().to_string(),
                        "fingerprint_sha256": outcome.fingerprint_sha256,
                        "thumbprint_sha1": thumbprint,
                    })
                );
            } else {
                println!(
                    "{action} cert={} fingerprint_sha256={} thumbprint_sha1={thumbprint}",
                    outcome.certificate.display(),
                    outcome.fingerprint_sha256
                );
            }
        }
        _ => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "platform": outcome.platform,
                        "backend": outcome.backend,
                        "action": action,
                        "cert": outcome.certificate.display().to_string(),
                        "fingerprint_sha256": outcome.fingerprint_sha256,
                    })
                );
            } else {
                println!(
                    "{action} cert={} fingerprint_sha256={}",
                    outcome.certificate.display(),
                    outcome.fingerprint_sha256
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_leaf_host(host: &str) -> CliResult<()> {
    if host.trim().is_empty() || host.contains('/') || host.chars().any(char::is_whitespace) {
        return Err(CliError::Usage(format!(
            "invalid certificate host `{host}`"
        )));
    }
    Ok(())
}

pub(super) fn display_ca_command_args(args: &[String]) -> String {
    args.iter()
        .map(|argument| {
            if argument.is_empty() {
                "\"\"".to_string()
            } else if argument.chars().any(char::is_whitespace) {
                format!("{argument:?}")
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
