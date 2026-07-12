use super::*;

pub(in crate::cli) fn linux_system_proxy_status(args: &[String]) -> Result<(), String> {
    let commands = linux_proxy_status_commands();
    if has_flag(args, "--dry-run") {
        let lines = linux_proxy_status_dry_run_lines();
        print_proxy_plan("linux", &lines, args);
        return Ok(());
    }
    linux_native_status(args, &commands)
}

pub(in crate::cli) fn linux_system_proxy_set(args: &[String], enabled: bool) -> Result<(), String> {
    let (host, port) = proxy_target(args)?;
    let bypass = proxy_bypass_domains(args);
    if has_flag(args, "--dry-run") {
        let lines = linux_proxy_set_dry_run_lines(enabled, &host, port, bypass.as_deref());
        print_proxy_plan("linux", &lines, args);
        if !has_flag(args, "--json") {
            println!(
                "proxy_{} platform=linux host={} port={}",
                if enabled { "on" } else { "off" },
                host,
                port
            );
        }
        return Ok(());
    }
    linux_native_set(args, enabled, &host, port, bypass.as_deref())
}

pub(in crate::cli) fn linux_proxy_status_dry_run_lines() -> Vec<String> {
    linux_proxy_status_commands()
        .iter()
        .map(|args| format!("dry-run linux gsettings {}", display_command_args(args)))
        .collect()
}

pub(in crate::cli) fn linux_proxy_set_dry_run_lines(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<String> {
    if enabled {
        let port = port.to_string();
        let mut lines = vec![
            linux_gsettings_line(&["set", "org.gnome.system.proxy", "mode", "manual"]),
            linux_gsettings_line(&["set", "org.gnome.system.proxy.http", "host", host]),
            linux_gsettings_line(&["set", "org.gnome.system.proxy.http", "port", &port]),
            linux_gsettings_line(&["set", "org.gnome.system.proxy.https", "host", host]),
            linux_gsettings_line(&["set", "org.gnome.system.proxy.https", "port", &port]),
        ];
        if let Some(domains) = bypass {
            let ignore_hosts = linux_ignore_hosts_value(domains);
            lines.push(linux_gsettings_line(&[
                "set",
                "org.gnome.system.proxy",
                "ignore-hosts",
                &ignore_hosts,
            ]));
        }
        lines.push(format!(
            "dry-run linux env export http_proxy=http://{host}:{port} https_proxy=http://{host}:{port} all_proxy=http://{host}:{port}"
        ));
        lines
    } else {
        vec![
            linux_gsettings_line(&["set", "org.gnome.system.proxy", "mode", "none"]),
            "dry-run linux env unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY".to_string(),
        ]
    }
}

pub(in crate::cli) fn linux_gsettings_line(args: &[&str]) -> String {
    format!(
        "dry-run linux gsettings {}",
        display_command_args(&args.iter().map(|item| item.to_string()).collect::<Vec<_>>())
    )
}

pub(in crate::cli) fn linux_ignore_hosts_value(domains: &[String]) -> String {
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

fn linux_proxy_status_commands() -> Vec<Vec<String>> {
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
fn linux_proxy_set_commands(
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Vec<Vec<String>> {
    if !enabled {
        return vec![strings(&["set", "org.gnome.system.proxy", "mode", "none"])];
    }
    let port = port.to_string();
    let mut commands = vec![
        strings(&["set", "org.gnome.system.proxy", "mode", "manual"]),
        strings(&["set", "org.gnome.system.proxy.http", "host", host]),
        strings(&["set", "org.gnome.system.proxy.http", "port", &port]),
        strings(&["set", "org.gnome.system.proxy.https", "host", host]),
        strings(&["set", "org.gnome.system.proxy.https", "port", &port]),
    ];
    if let Some(domains) = bypass {
        commands.push(vec![
            "set".to_string(),
            "org.gnome.system.proxy".to_string(),
            "ignore-hosts".to_string(),
            linux_ignore_hosts_value(domains),
        ]);
    }
    commands
}

#[cfg(target_os = "linux")]
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[cfg(target_os = "linux")]
fn linux_native_status(args: &[String], commands: &[Vec<String>]) -> Result<(), String> {
    let settings = commands
        .iter()
        .map(|command| {
            let output = platform_command_output("gsettings", command)?;
            Ok(serde_json::json!({
                "schema": command[1],
                "key": command[2],
                "value": String::from_utf8_lossy(&output.stdout).trim(),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({"platform": "linux", "backend": "gsettings", "settings": settings})
        );
    } else {
        for setting in settings {
            println!(
                "{}.{}={}",
                setting["schema"].as_str().unwrap_or_default(),
                setting["key"].as_str().unwrap_or_default(),
                setting["value"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn linux_native_status(_args: &[String], _commands: &[Vec<String>]) -> Result<(), String> {
    Err(
        "native Linux system proxy access requires a Linux build; use --dry-run elsewhere"
            .to_string(),
    )
}

#[cfg(target_os = "linux")]
fn linux_native_set(
    args: &[String],
    enabled: bool,
    host: &str,
    port: u16,
    bypass: Option<&[String]>,
) -> Result<(), String> {
    let commands = linux_proxy_set_commands(enabled, host, port, bypass);
    let mut applied: Vec<(String, String, String)> = Vec::new();
    for command in &commands {
        let schema = command[1].clone();
        let key = command[2].clone();
        let previous = linux_gsettings_get(&schema, &key)?;
        applied.push((schema, key, previous));
        if let Err(error) = platform_command_output("gsettings", command) {
            linux_rollback(&applied);
            return Err(error);
        }
    }
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "linux",
                "backend": "gsettings",
                "enabled": enabled,
                "host": host,
                "port": port,
                "bypass": bypass,
            })
        );
    } else {
        println!(
            "proxy_{} platform=linux host={} port={}",
            if enabled { "on" } else { "off" },
            host,
            port
        );
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn linux_native_set(
    _args: &[String],
    _enabled: bool,
    _host: &str,
    _port: u16,
    _bypass: Option<&[String]>,
) -> Result<(), String> {
    Err(
        "native Linux system proxy changes require a Linux build; use --dry-run elsewhere"
            .to_string(),
    )
}

#[cfg(target_os = "linux")]
fn linux_gsettings_get(schema: &str, key: &str) -> Result<String, String> {
    let output = platform_command_output(
        "gsettings",
        &["get".to_string(), schema.to_string(), key.to_string()],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn linux_rollback(applied: &[(String, String, String)]) {
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
