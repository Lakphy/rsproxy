use super::*;

pub(super) fn plan(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyPlan> {
    match action.enabled() {
        None => Ok(status_plan()),
        Some(enabled) => set_plan(options, enabled),
    }
}

pub(super) fn execute(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyOutcome> {
    match action.enabled() {
        None => native_status(&proxy_status_commands()),
        Some(enabled) => {
            let target = required_target(options)?;
            native_set(enabled, target, options.bypass.as_deref())
        }
    }
}

fn status_plan() -> ProxyPlan {
    let steps = proxy_status_commands()
        .into_iter()
        .map(|args| ProxyPlanStep::Command(ProxyCommand::LinuxGsettings { args }))
        .collect();
    ProxyPlan::new(ProxyPlatform::Linux, steps)
}

fn set_plan(options: &ProxyOptions, enabled: bool) -> PlatformResult<ProxyPlan> {
    let target = required_target(options)?;
    let mut steps = proxy_set_plan_commands(
        enabled,
        &target.host,
        target.port,
        options.bypass.as_deref(),
    )
    .into_iter()
    .map(ProxyPlanStep::Command)
    .collect::<Vec<_>>();
    steps.push(ProxyPlanStep::Change(ProxyChange {
        platform: ProxyPlatform::Linux,
        enabled,
        target: target.clone(),
        bypass: options.bypass.clone(),
        service: None,
    }));
    Ok(ProxyPlan::new(ProxyPlatform::Linux, steps))
}

pub(super) fn proxy_set_plan_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<ProxyCommand> {
    if enabled {
        let port = port.to_string();
        let mut commands = vec![
            linux_gsettings(&["set", "org.gnome.system.proxy", "mode", "manual"]),
            linux_gsettings(&["set", "org.gnome.system.proxy.http", "host", host]),
            linux_gsettings(&["set", "org.gnome.system.proxy.http", "port", &port]),
            linux_gsettings(&["set", "org.gnome.system.proxy.https", "host", host]),
            linux_gsettings(&["set", "org.gnome.system.proxy.https", "port", &port]),
        ];
        if let Some(domains) = bypass {
            let ignore_hosts = ignore_hosts_value(domains);
            commands.push(linux_gsettings(&[
                "set",
                "org.gnome.system.proxy",
                "ignore-hosts",
                &ignore_hosts,
            ]));
        }
        commands.push(ProxyCommand::LinuxEnvironment {
            args: vec![
                "export".to_string(),
                format!("http_proxy=http://{host}:{port}"),
                format!("https_proxy=http://{host}:{port}"),
                format!("all_proxy=http://{host}:{port}"),
            ],
        });
        commands
    } else {
        vec![
            linux_gsettings(&["set", "org.gnome.system.proxy", "mode", "none"]),
            ProxyCommand::LinuxEnvironment {
                args: [
                    "unset",
                    "http_proxy",
                    "https_proxy",
                    "all_proxy",
                    "HTTP_PROXY",
                    "HTTPS_PROXY",
                    "ALL_PROXY",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ]
    }
}

fn linux_gsettings(args: &[&str]) -> ProxyCommand {
    ProxyCommand::LinuxGsettings {
        args: args.iter().map(|item| item.to_string()).collect(),
    }
}

fn ignore_hosts_value(domains: &[String]) -> String {
    if domains.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            domains
                .iter()
                .map(|domain| format!("'{}'", domain.replace('\'', "\\'")))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn proxy_status_commands() -> Vec<Vec<String>> {
    [
        ["get", "org.gnome.system.proxy", "mode"],
        ["get", "org.gnome.system.proxy.http", "host"],
        ["get", "org.gnome.system.proxy.http", "port"],
        ["get", "org.gnome.system.proxy.https", "host"],
        ["get", "org.gnome.system.proxy.https", "port"],
        ["get", "org.gnome.system.proxy", "ignore-hosts"],
    ]
    .iter()
    .map(|args| args.iter().map(|arg| (*arg).to_string()).collect())
    .collect()
}

#[cfg(target_os = "linux")]
fn proxy_set_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<Vec<String>> {
    proxy_set_plan_commands(enabled, host, port, bypass)
        .into_iter()
        .filter_map(|command| match command {
            ProxyCommand::LinuxGsettings { args } => Some(args),
            // Environment changes are shell guidance: a child process cannot mutate the
            // invoking shell. Native execution deliberately applies only gsettings steps.
            ProxyCommand::LinuxEnvironment { .. } => None,
            ProxyCommand::MacosNetworkSetup { .. } | ProxyCommand::WindowsRegistry { .. } => {
                unreachable!("Linux proxy plans only contain Linux commands")
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn native_status(commands: &[Vec<String>]) -> PlatformResult<ProxyOutcome> {
    let settings = commands
        .iter()
        .map(|command| {
            let output = platform_command_output("gsettings", command)?;
            Ok(LinuxSettingStatus {
                schema: command[1].clone(),
                key: command[2].clone(),
                value: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            })
        })
        .collect::<PlatformResult<Vec<_>>>()?;
    Ok(ProxyOutcome::Status(ProxyStatus::Linux { settings }))
}

#[cfg(not(target_os = "linux"))]
fn native_status(_commands: &[Vec<String>]) -> PlatformResult<ProxyOutcome> {
    Err(PlatformError::Unsupported(
        "native Linux system proxy access requires a Linux build; use --dry-run elsewhere"
            .to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn native_set(
    enabled: bool,
    target: &ProxyTarget,
    bypass: Option<&[String]>,
) -> PlatformResult<ProxyOutcome> {
    let commands = proxy_set_commands(enabled, &target.host, target.port, bypass);
    let mut applied: Vec<(String, String, String)> = Vec::new();
    for command in &commands {
        let schema = command[1].clone();
        let key = command[2].clone();
        let previous = gsettings_get(&schema, &key)?;
        applied.push((schema, key, previous));
        if let Err(error) = platform_command_output("gsettings", command) {
            rollback(&applied);
            return Err(error);
        }
    }
    Ok(ProxyOutcome::Changed(vec![ProxyChange {
        platform: ProxyPlatform::Linux,
        enabled,
        target: target.clone(),
        bypass: bypass.map(<[String]>::to_vec),
        service: None,
    }]))
}

#[cfg(not(target_os = "linux"))]
fn native_set(
    _enabled: bool,
    _target: &ProxyTarget,
    _bypass: Option<&[String]>,
) -> PlatformResult<ProxyOutcome> {
    Err(PlatformError::Unsupported(
        "native Linux system proxy changes require a Linux build; use --dry-run elsewhere"
            .to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> PlatformResult<String> {
    let output = platform_command_output(
        "gsettings",
        &["get".to_string(), schema.to_string(), key.to_string()],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn rollback(applied: &[(String, String, String)]) {
    for (schema, key, value) in applied.iter().rev() {
        let _ = platform_command_output(
            "gsettings",
            &[
                "set".to_string(),
                schema.clone(),
                key.clone(),
                value.clone(),
            ],
        );
    }
}
