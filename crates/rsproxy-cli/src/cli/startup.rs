use super::command::{ClientArgs, RuntimeArgs, StartupInstallArgs, StartupUninstallArgs};
use super::{config, daemon, system_proxy};
use crate::{CliError, CliResult, DaemonConflict};
use rsproxy_platform::startup::{
    StartupPlatform, StartupRegistration, StartupStatus, install_startup, startup_manifest_path,
    startup_status, uninstall_startup,
};
use rsproxy_platform::system_proxy::{ProxyAction, ProxyTarget};
use std::env;
use std::path::{Path, PathBuf};

mod manifest;

use manifest::{
    STARTUP_MANIFEST_VERSION, StartupManifest, read_manifest_lenient, read_manifest_required,
    remove_manifest, write_manifest,
};

pub(super) fn install(args: StartupInstallArgs, json: bool) -> CliResult<()> {
    if args.no_system_proxy && (args.service.is_some() || args.bypass.is_some()) {
        return Err(CliError::Usage(
            "--service and --bypass require automatic system proxy routing; remove --no-system-proxy"
                .to_string(),
        ));
    }

    let manifest = resolve_manifest(&args)?;
    let manifest_path = startup_manifest_path()?;
    let executable = env::current_exe()
        .map_err(|source| CliError::io("resolve rsproxy executable for startup", source))?;
    let registration = StartupRegistration {
        executable,
        arguments: vec!["startup".to_string(), "launch".to_string()],
    };

    if args.dry_run {
        let before = startup_status()?;
        render_install(
            &before,
            &manifest_path,
            &manifest,
            args.start_now,
            true,
            json,
        )?;
        return Ok(());
    }

    let (previous, _) = read_manifest_lenient(&manifest_path);
    // Routing enabled under the previous manifest's scope is orphaned once the scope narrows
    // (e.g. all services -> one service, or system proxy turned off), because uninstall will
    // only ever disable the scope recorded in the manifest it finds.
    if let Some(previous) = &previous
        && previous.system_proxy
        && (!manifest.system_proxy || previous.service != manifest.service)
    {
        disable_routing_best_effort(
            previous,
            json,
            "could not restore previous system proxy scope",
        );
    }
    write_manifest(&manifest)?;
    let installed = match install_startup(&registration) {
        Ok(status) => status,
        Err(error) => {
            // A manifest for a registration that was never created would make a later
            // `startup uninstall` mutate routing and daemon state the feature never touched.
            match &previous {
                Some(previous) => {
                    let _ = write_manifest(previous);
                }
                None => {
                    let _ = remove_manifest();
                }
            }
            return Err(error.into());
        }
    };
    if args.start_now {
        launch_manifest(&manifest, !json)?;
    }
    render_install(
        &installed,
        &manifest_path,
        &manifest,
        args.start_now,
        false,
        json,
    )
}

pub(super) fn status(json: bool) -> CliResult<()> {
    let native = startup_status()?;
    let manifest_path = startup_manifest_path()?;
    let (manifest, manifest_warning) = read_manifest_lenient(&manifest_path);
    if let Some(warning) = &manifest_warning {
        eprintln!("warning: {warning}");
    }
    if json {
        println!(
            "{}",
            serde_json::json!({
                "platform": platform_name(native.platform),
                "installed": native.installed,
                "location": native.location,
                "manifest": manifest_path,
                "configured": manifest.is_some(),
                "system_proxy": manifest.as_ref().map(|value| value.system_proxy),
                "storage": manifest.as_ref().map(|value| &value.storage),
                "config": manifest.as_ref().and_then(|value| value.config.as_ref()),
            })
        );
    } else {
        println!(
            "startup installed={} platform={} location={}",
            native.installed,
            platform_name(native.platform),
            native.location
        );
        println!(
            "manifest configured={} path={}",
            manifest.is_some(),
            manifest_path.display()
        );
        if let Some(manifest) = manifest {
            println!(
                "runtime storage={} config={} system_proxy={}",
                manifest.storage.display(),
                manifest
                    .config
                    .as_ref()
                    .map_or_else(|| "defaults".to_string(), |path| path.display().to_string()),
                if manifest.system_proxy { "on" } else { "off" }
            );
        }
    }
    Ok(())
}

pub(super) fn uninstall(args: StartupUninstallArgs, json: bool) -> CliResult<()> {
    let native = startup_status()?;
    let manifest_path = startup_manifest_path()?;
    let (manifest, manifest_warning) = read_manifest_lenient(&manifest_path);
    if let Some(warning) = &manifest_warning {
        eprintln!("warning: {warning}");
    }

    // Checked before the dry-run branch so the preview and the real run agree.
    if !args.keep_running && native.installed && manifest.is_none() {
        return Err(CliError::Usage(format!(
            "startup manifest {} is missing or unreadable; use --keep-running to remove only the broken registration",
            manifest_path.display()
        )));
    }

    if args.dry_run {
        render_uninstall(
            &native,
            &manifest_path,
            manifest.as_ref(),
            args.keep_running,
            true,
            json,
        )?;
        return Ok(());
    }

    // Runtime cleanup is best effort: a failed proxy restore or daemon stop must not leave the
    // login item installed, or the next login re-creates the state the user is removing.
    if !args.keep_running
        && let Some(manifest) = &manifest
    {
        if manifest.system_proxy {
            disable_routing_best_effort(manifest, json, "could not restore system proxy");
        }
        match daemon::stop_server(&stop_args(manifest), !json) {
            Ok(()) | Err(CliError::DaemonConflict(DaemonConflict::NotRunning { .. })) => {}
            Err(error) => eprintln!("warning: could not stop the daemon: {error}"),
        }
    }

    uninstall_startup()?;
    remove_manifest()?;
    render_uninstall(
        &native,
        &manifest_path,
        manifest.as_ref(),
        args.keep_running,
        false,
        json,
    )
}

pub(super) fn launch(json: bool) -> CliResult<()> {
    let path = startup_manifest_path()?;
    let manifest = read_manifest_required(&path)?;
    let proxy_routing = launch_manifest(&manifest, !json)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": "launch",
                "system_proxy": manifest.system_proxy,
                "proxy_routing": proxy_routing,
            })
        );
    }
    Ok(())
}

fn launch_manifest(manifest: &StartupManifest, announce: bool) -> CliResult<Vec<String>> {
    let runtime = runtime_args(manifest);
    match daemon::start_server(&runtime, announce) {
        Ok(()) | Err(CliError::DaemonConflict(DaemonConflict::AlreadyRunning { .. })) => {}
        Err(error) => return Err(error),
    }
    let mut proxy_routing = Vec::new();
    if manifest.system_proxy {
        // An already-running daemon may listen on a different port than the manifest's config
        // resolves to (earlier CLI override), so route to the address it actually serves.
        let target =
            daemon::live_proxy_address(&runtime).map(|(host, port)| ProxyTarget { host, port })?;
        for line in enable_system_proxy(manifest, target)? {
            if announce {
                println!("{line}");
            }
            proxy_routing.push(line);
        }
    }
    Ok(proxy_routing)
}

fn resolve_manifest(args: &StartupInstallArgs) -> CliResult<StartupManifest> {
    let client = ClientArgs {
        storage: args.storage.clone(),
        config: args.config.clone(),
        ..ClientArgs::default()
    };
    let runtime = RuntimeArgs::from_client(client);
    let resolved = config::runtime_config(&runtime)?;
    // A configuration only valid for `run` (ephemeral port 0) would register a login item whose
    // launch fails at every login, so enforce the daemon-mode constraints at install time.
    daemon::validate_daemon_addresses(&resolved)?;
    let storage = absolute_path(&resolved.engine().storage)?;
    let config = resolved
        .config_path
        .as_deref()
        .map(absolute_path)
        .transpose()?;
    Ok(StartupManifest {
        version: STARTUP_MANIFEST_VERSION,
        storage,
        config,
        system_proxy: !args.no_system_proxy,
        service: args.service.clone(),
        bypass: system_proxy::parse_bypass_list(args.bypass.as_deref()),
        proxy_host: resolved.host.clone(),
        proxy_port: resolved.port,
    })
}

fn runtime_args(manifest: &StartupManifest) -> RuntimeArgs {
    RuntimeArgs::from_client(ClientArgs {
        storage: Some(manifest.storage.clone()),
        config: manifest.config.clone(),
        ..ClientArgs::default()
    })
}

fn stop_args(manifest: &StartupManifest) -> RuntimeArgs {
    let mut client = ClientArgs {
        storage: Some(manifest.storage.clone()),
        config: manifest.config.clone(),
        ..ClientArgs::default()
    };
    // A config file deleted after install must not block stopping the daemon during uninstall;
    // the pidfile lives under the storage directory recorded in the manifest.
    if client.config.as_ref().is_some_and(|path| !path.exists()) {
        client.config = None;
    }
    RuntimeArgs::from_client(client)
}

fn enable_system_proxy(manifest: &StartupManifest, target: ProxyTarget) -> CliResult<Vec<String>> {
    system_proxy::automatic_system_proxy(
        ProxyAction::Enable,
        target,
        manifest.service.clone(),
        manifest.bypass.clone(),
    )
}

/// The disable commands never use the target (it only labels the report), so the manifest's
/// recorded listener serves even when the config file it came from is gone.
fn disable_system_proxy(manifest: &StartupManifest) -> CliResult<Vec<String>> {
    system_proxy::automatic_system_proxy(
        ProxyAction::Disable,
        ProxyTarget {
            host: manifest.proxy_host.clone(),
            port: manifest.proxy_port,
        },
        manifest.service.clone(),
        manifest.bypass.clone(),
    )
}

fn disable_routing_best_effort(manifest: &StartupManifest, json: bool, warning: &str) {
    match disable_system_proxy(manifest) {
        Ok(lines) => {
            if !json {
                for line in lines {
                    println!("{line}");
                }
            }
        }
        Err(error) => eprintln!("warning: {warning}: {error}"),
    }
}

fn absolute_path(path: &Path) -> CliResult<PathBuf> {
    std::path::absolute(path)
        .map_err(|source| CliError::io("resolve absolute startup path", source))
}

fn render_install(
    native: &StartupStatus,
    manifest_path: &Path,
    manifest: &StartupManifest,
    start_now: bool,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": "install",
                "dry_run": dry_run,
                "platform": platform_name(native.platform),
                "location": native.location,
                "manifest": manifest_path,
                "system_proxy": manifest.system_proxy,
                "service": manifest.service,
                "start_now": start_now,
            })
        );
    } else {
        println!(
            "{} startup platform={} location={}",
            if dry_run {
                "would install"
            } else {
                "installed"
            },
            platform_name(native.platform),
            native.location
        );
        println!(
            "manifest={} system_proxy={} start_now={}",
            manifest_path.display(),
            if manifest.system_proxy { "on" } else { "off" },
            start_now
        );
    }
    Ok(())
}

fn render_uninstall(
    native: &StartupStatus,
    manifest_path: &Path,
    manifest: Option<&StartupManifest>,
    keep_running: bool,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": "uninstall",
                "dry_run": dry_run,
                "platform": platform_name(native.platform),
                "location": native.location,
                "manifest": manifest_path,
                "was_installed": native.installed,
                "stop_runtime": !keep_running,
                "restore_system_proxy": !keep_running && manifest.is_some_and(|value| value.system_proxy),
            })
        );
    } else {
        println!(
            "{} startup platform={} location={}",
            if dry_run {
                "would uninstall"
            } else {
                "uninstalled"
            },
            platform_name(native.platform),
            native.location
        );
        println!(
            "runtime_cleanup={} system_proxy_restore={} manifest={}",
            !keep_running,
            !keep_running && manifest.is_some_and(|value| value.system_proxy),
            manifest_path.display()
        );
    }
    Ok(())
}

fn platform_name(platform: StartupPlatform) -> &'static str {
    match platform {
        StartupPlatform::Macos => "macos-launch-agent",
        StartupPlatform::Windows => "windows-run-key",
        StartupPlatform::Linux => "xdg-autostart",
    }
}

#[cfg(test)]
mod tests;
