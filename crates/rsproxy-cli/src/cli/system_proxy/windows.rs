use super::*;

pub(in crate::cli) fn windows_system_proxy_status(args: &[String]) -> Result<(), String> {
    if has_flag(args, "--dry-run") {
        let lines = windows_proxy_status_dry_run_lines();
        print_proxy_plan("windows", &lines, args);
        return Ok(());
    }
    windows_native_status(args)
}

pub(in crate::cli) fn windows_system_proxy_set(
    args: &[String],
    enabled: bool,
) -> Result<(), String> {
    let (host, port) = proxy_target(args)?;
    let bypass = proxy_bypass_domains(args);
    if has_flag(args, "--dry-run") {
        let lines = windows_proxy_set_dry_run_lines(enabled, &host, port, bypass.as_deref());
        print_proxy_plan("windows", &lines, args);
        if !has_flag(args, "--json") {
            println!(
                "proxy_{} platform=windows host={} port={}",
                if enabled { "on" } else { "off" },
                host,
                port
            );
        }
        return Ok(());
    }
    windows_native_set(args, enabled, &host, port, bypass.as_deref())
}

pub(in crate::cli) fn windows_proxy_status_dry_run_lines() -> Vec<String> {
    let key = windows_internet_settings_key();
    ["ProxyEnable", "ProxyServer", "ProxyOverride"]
        .iter()
        .map(|value| {
            format!(
                "dry-run windows reg query {}",
                display_command_args(&[key.to_string(), "/v".to_string(), (*value).to_string(),])
            )
        })
        .collect()
}

pub(in crate::cli) fn windows_proxy_set_dry_run_lines(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<String> {
    let key = windows_internet_settings_key();
    if enabled {
        let proxy_server = format!("http={host}:{port};https={host}:{port}");
        let mut lines = vec![
            windows_reg_add_line(key, "ProxyEnable", "REG_DWORD", "1"),
            windows_reg_add_line(key, "ProxyServer", "REG_SZ", &proxy_server),
        ];
        if let Some(domains) = bypass {
            let override_value = windows_bypass_value(domains);
            lines.push(windows_reg_add_line(
                key,
                "ProxyOverride",
                "REG_SZ",
                &override_value,
            ));
        }
        lines
    } else {
        vec![
            windows_reg_add_line(key, "ProxyEnable", "REG_DWORD", "0"),
            format!(
                "dry-run windows reg delete {}",
                display_command_args(&[
                    key.to_string(),
                    "/v".to_string(),
                    "ProxyServer".to_string(),
                    "/f".to_string(),
                ])
            ),
            format!(
                "dry-run windows reg delete {}",
                display_command_args(&[
                    key.to_string(),
                    "/v".to_string(),
                    "ProxyOverride".to_string(),
                    "/f".to_string(),
                ])
            ),
        ]
    }
}

pub(in crate::cli) fn windows_internet_settings_key() -> &'static str {
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
}

pub(in crate::cli) fn windows_reg_add_line(
    key: &str,
    value: &str,
    value_type: &str,
    data: &str,
) -> String {
    format!(
        "dry-run windows reg add {}",
        display_command_args(&[
            key.to_string(),
            "/v".to_string(),
            value.to_string(),
            "/t".to_string(),
            value_type.to_string(),
            "/d".to_string(),
            data.to_string(),
            "/f".to_string(),
        ])
    )
}

pub(in crate::cli) fn windows_bypass_value(domains: &[String]) -> String {
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
fn windows_native_status(args: &[String]) -> Result<(), String> {
    let values = windows_query_values()?;
    let enabled = values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case("ProxyEnable"))
        .is_some_and(|value| value.data == "0x1" || value.data == "1");
    let server = windows_value_data(&values, "ProxyServer");
    let bypass = windows_value_data(&values, "ProxyOverride");
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "windows",
                "backend": "wininet-registry",
                "enabled": enabled,
                "server": server,
                "bypass": bypass,
            })
        );
    } else {
        println!("enabled={enabled}");
        println!("server={}", server.as_deref().unwrap_or("-"));
        println!("bypass={}", bypass.as_deref().unwrap_or("-"));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn windows_native_status(_args: &[String]) -> Result<(), String> {
    Err(
        "native Windows system proxy access requires a Windows build; use --dry-run elsewhere"
            .to_string(),
    )
}

#[cfg(target_os = "windows")]
fn windows_native_set(
    args: &[String],
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Result<(), String> {
    let previous = windows_query_values()?;
    let commands = windows_native_set_commands(enabled, host, port, bypass, &previous);
    for command in &commands {
        if let Err(error) = platform_command_output("reg", command) {
            windows_restore_values(&previous);
            return Err(error);
        }
    }
    if let Err(error) = windows_notify_proxy_change() {
        windows_restore_values(&previous);
        let _ = windows_notify_proxy_change();
        return Err(error);
    }
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "windows",
                "backend": "wininet-registry",
                "enabled": enabled,
                "host": host,
                "port": port,
                "bypass": bypass,
            })
        );
    } else {
        println!(
            "proxy_{} platform=windows host={} port={}",
            if enabled { "on" } else { "off" },
            host,
            port
        );
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn windows_native_set(
    _args: &[String],
    _enabled: bool,
    _host: &str,
    _port: u16,
    _bypass: Option<&[String]>,
) -> Result<(), String> {
    Err(
        "native Windows system proxy changes require a Windows build; use --dry-run elsewhere"
            .to_string(),
    )
}

#[cfg(target_os = "windows")]
fn windows_native_set_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
    previous: &[WindowsRegistryValue],
) -> Vec<Vec<String>> {
    let key = windows_internet_settings_key();
    let mut commands = vec![windows_reg_add_args(
        key,
        "ProxyEnable",
        "REG_DWORD",
        if enabled { "1" } else { "0" },
    )];
    if enabled {
        commands.push(windows_reg_add_args(
            key,
            "ProxyServer",
            "REG_SZ",
            &format!("http={host}:{port};https={host}:{port}"),
        ));
        if let Some(domains) = bypass {
            commands.push(windows_reg_add_args(
                key,
                "ProxyOverride",
                "REG_SZ",
                &windows_bypass_value(domains),
            ));
        }
    } else {
        for name in ["ProxyServer", "ProxyOverride"] {
            if previous
                .iter()
                .any(|value| value.name.eq_ignore_ascii_case(name))
            {
                commands.push(windows_reg_delete_args(key, name));
            }
        }
    }
    commands
}

#[cfg(target_os = "windows")]
fn windows_query_values() -> Result<Vec<WindowsRegistryValue>, String> {
    let output = platform_command_output(
        "reg",
        &[
            "query".to_string(),
            windows_internet_settings_key().to_string(),
        ],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(windows_parse_registry_line)
        .filter(|value| {
            ["ProxyEnable", "ProxyServer", "ProxyOverride"]
                .iter()
                .any(|name| value.name.eq_ignore_ascii_case(name))
        })
        .collect())
}

#[cfg(target_os = "windows")]
fn windows_parse_registry_line(line: &str) -> Option<WindowsRegistryValue> {
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
fn windows_value_data(values: &[WindowsRegistryValue], name: &str) -> Option<String> {
    values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case(name))
        .map(|value| value.data.clone())
}

#[cfg(target_os = "windows")]
fn windows_restore_values(previous: &[WindowsRegistryValue]) {
    let key = windows_internet_settings_key();
    for name in ["ProxyEnable", "ProxyServer", "ProxyOverride"] {
        let command = previous
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case(name))
            .map(|value| windows_reg_add_args(key, name, &value.value_type, &value.data))
            .unwrap_or_else(|| windows_reg_delete_args(key, name));
        let _ = platform_command_output("reg", &command);
    }
}

#[cfg(target_os = "windows")]
fn windows_reg_add_args(key: &str, name: &str, value_type: &str, data: &str) -> Vec<String> {
    ["add", key, "/v", name, "/t", value_type, "/d", data, "/f"]
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_reg_delete_args(key: &str, name: &str) -> Vec<String> {
    ["delete", key, "/v", name, "/f"]
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_notify_proxy_change() -> Result<(), String> {
    use windows_sys::Win32::Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    };

    for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
        let result =
            unsafe { InternetSetOptionW(std::ptr::null_mut(), option, std::ptr::null(), 0) };
        if result == 0 {
            return Err(format!(
                "notify WinINet proxy change: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}
