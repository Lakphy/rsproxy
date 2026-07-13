use super::*;

pub(super) fn plan(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyPlan> {
    match action.enabled() {
        None => Ok(status_plan()),
        Some(enabled) => set_plan(options, enabled),
    }
}

pub(super) fn execute(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyOutcome> {
    match action.enabled() {
        None => native_status(),
        Some(enabled) => {
            let target = required_target(options)?;
            native_set(enabled, target, options.bypass.as_deref())
        }
    }
}

fn status_plan() -> ProxyPlan {
    let steps = proxy_status_plan_commands()
        .into_iter()
        .map(ProxyPlanStep::Command)
        .collect();
    ProxyPlan::new(ProxyPlatform::Windows, steps)
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
        platform: ProxyPlatform::Windows,
        enabled,
        target: target.clone(),
        bypass: options.bypass.clone(),
        service: None,
    }));
    Ok(ProxyPlan::new(ProxyPlatform::Windows, steps))
}

fn proxy_status_plan_commands() -> Vec<ProxyCommand> {
    let key = internet_settings_key();
    ["ProxyEnable", "ProxyServer", "ProxyOverride"]
        .iter()
        .map(|value| ProxyCommand::WindowsRegistry {
            args: vec![
                "query".to_string(),
                key.to_string(),
                "/v".to_string(),
                (*value).to_string(),
            ],
        })
        .collect()
}

pub(super) fn proxy_set_plan_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<ProxyCommand> {
    let key = internet_settings_key();
    if enabled {
        let proxy_server = format!("http={host}:{port};https={host}:{port}");
        let mut commands = vec![
            reg_add_command(key, "ProxyEnable", "REG_DWORD", "1"),
            reg_add_command(key, "ProxyServer", "REG_SZ", &proxy_server),
        ];
        if let Some(domains) = bypass {
            commands.push(reg_add_command(
                key,
                "ProxyOverride",
                "REG_SZ",
                &bypass_value(domains),
            ));
        }
        commands
    } else {
        vec![
            reg_add_command(key, "ProxyEnable", "REG_DWORD", "0"),
            ProxyCommand::WindowsRegistry {
                args: vec![
                    "delete".to_string(),
                    key.to_string(),
                    "/v".to_string(),
                    "ProxyServer".to_string(),
                    "/f".to_string(),
                ],
            },
            ProxyCommand::WindowsRegistry {
                args: vec![
                    "delete".to_string(),
                    key.to_string(),
                    "/v".to_string(),
                    "ProxyOverride".to_string(),
                    "/f".to_string(),
                ],
            },
        ]
    }
}

fn internet_settings_key() -> &'static str {
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
}

fn reg_add_command(key: &str, value: &str, value_type: &str, data: &str) -> ProxyCommand {
    ProxyCommand::WindowsRegistry {
        args: ["add", key, "/v", value, "/t", value_type, "/d", data, "/f"]
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    }
}

fn bypass_value(domains: &[String]) -> String {
    if domains.is_empty() {
        "<local>".to_string()
    } else {
        domains.join(";")
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct WindowsRegistryValue {
    name: String,
    value_type: String,
    data: String,
}

#[cfg(target_os = "windows")]
fn native_status() -> PlatformResult<ProxyOutcome> {
    let values = query_values()?;
    let enabled = values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case("ProxyEnable"))
        .is_some_and(|value| value.data == "0x1" || value.data == "1");
    let server = value_data(&values, "ProxyServer");
    let bypass = value_data(&values, "ProxyOverride");
    Ok(ProxyOutcome::Status(ProxyStatus::Windows {
        enabled,
        server,
        bypass,
    }))
}

#[cfg(not(target_os = "windows"))]
fn native_status() -> PlatformResult<ProxyOutcome> {
    Err(PlatformError::Unsupported(
        "native Windows system proxy access requires a Windows build; use --dry-run elsewhere"
            .to_string(),
    ))
}

#[cfg(target_os = "windows")]
fn native_set(
    enabled: bool,
    target: &ProxyTarget,
    bypass: Option<&[String]>,
) -> PlatformResult<ProxyOutcome> {
    let previous = query_values()?;
    let commands = native_set_commands(enabled, &target.host, target.port, bypass, &previous);
    for command in &commands {
        if let Err(error) = platform_command_output("reg", command) {
            restore_values(&previous);
            return Err(error);
        }
    }
    if let Err(error) = notify_proxy_change() {
        restore_values(&previous);
        let _ = notify_proxy_change();
        return Err(error);
    }
    Ok(ProxyOutcome::Changed(vec![ProxyChange {
        platform: ProxyPlatform::Windows,
        enabled,
        target: target.clone(),
        bypass: bypass.map(<[String]>::to_vec),
        service: None,
    }]))
}

#[cfg(not(target_os = "windows"))]
fn native_set(
    _enabled: bool,
    _target: &ProxyTarget,
    _bypass: Option<&[String]>,
) -> PlatformResult<ProxyOutcome> {
    Err(PlatformError::Unsupported(
        "native Windows system proxy changes require a Windows build; use --dry-run elsewhere"
            .to_string(),
    ))
}

#[cfg(target_os = "windows")]
fn native_set_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
    previous: &[WindowsRegistryValue],
) -> Vec<Vec<String>> {
    proxy_set_plan_commands(enabled, host, port, bypass)
        .into_iter()
        .map(|command| match command {
            ProxyCommand::WindowsRegistry { args } => args,
            ProxyCommand::MacosNetworkSetup { .. }
            | ProxyCommand::LinuxGsettings { .. }
            | ProxyCommand::LinuxEnvironment { .. } => {
                unreachable!("Windows proxy plans only contain registry commands")
            }
        })
        .filter(|args| {
            args.first().map(String::as_str) != Some("delete")
                || args.get(3).is_some_and(|name| {
                    previous
                        .iter()
                        .any(|value| value.name.eq_ignore_ascii_case(name))
                })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn query_values() -> PlatformResult<Vec<WindowsRegistryValue>> {
    let output = platform_command_output(
        "reg",
        &["query".to_string(), internet_settings_key().to_string()],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_registry_line)
        .filter(|value| {
            ["ProxyEnable", "ProxyServer", "ProxyOverride"]
                .iter()
                .any(|name| value.name.eq_ignore_ascii_case(name))
        })
        .collect())
}

#[cfg(target_os = "windows")]
fn parse_registry_line(line: &str) -> Option<WindowsRegistryValue> {
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let value_type = parts.next()?.to_string();
    if !value_type.starts_with("REG_") {
        return None;
    }
    let data = parts.collect::<Vec<_>>().join(" ");
    Some(WindowsRegistryValue {
        name,
        value_type,
        data,
    })
}

#[cfg(target_os = "windows")]
fn value_data(values: &[WindowsRegistryValue], name: &str) -> Option<String> {
    values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case(name))
        .map(|value| value.data.clone())
}

#[cfg(target_os = "windows")]
fn restore_values(previous: &[WindowsRegistryValue]) {
    let key = internet_settings_key();
    for name in ["ProxyEnable", "ProxyServer", "ProxyOverride"] {
        let command = previous
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case(name))
            .map(|value| reg_add_args(key, name, &value.value_type, &value.data))
            .unwrap_or_else(|| reg_delete_args(key, name));
        let _ = platform_command_output("reg", &command);
    }
}

#[cfg(target_os = "windows")]
fn reg_add_args(key: &str, name: &str, value_type: &str, data: &str) -> Vec<String> {
    ["add", key, "/v", name, "/t", value_type, "/d", data, "/f"]
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

#[cfg(target_os = "windows")]
fn reg_delete_args(key: &str, name: &str) -> Vec<String> {
    ["delete", key, "/v", name, "/f"]
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

#[cfg(target_os = "windows")]
fn notify_proxy_change() -> PlatformResult<()> {
    use windows_sys::Win32::Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    };

    for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
        // SAFETY: WinINet accepts null buffer and handle for these process-wide notification
        // options; the call does not dereference application-owned memory.
        let result =
            unsafe { InternetSetOptionW(std::ptr::null_mut(), option, std::ptr::null(), 0) };
        if result == 0 {
            return Err(PlatformError::Io {
                context: "notify WinINet proxy change".to_string(),
                source: std::io::Error::last_os_error(),
            });
        }
    }
    Ok(())
}
