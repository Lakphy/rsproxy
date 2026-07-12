#[cfg(target_os = "macos")]
use super::*;

#[cfg(target_os = "macos")]
pub(in crate::cli) fn macos_system_proxy_status(args: &[String]) -> Result<(), String> {
    let services = system_proxy_services(args, true)?;
    if has_flag(args, "--dry-run") {
        let lines = services
            .iter()
            .flat_map(|service| {
                [
                    vec!["-getwebproxy".to_string(), service.clone()],
                    vec!["-getsecurewebproxy".to_string(), service.clone()],
                    vec!["-getproxybypassdomains".to_string(), service.clone()],
                ]
                .into_iter()
                .map(|command| {
                    format!(
                        "dry-run macos networksetup {}",
                        display_command_args(&command)
                    )
                })
                .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        print_proxy_plan("macos", &lines, args);
        return Ok(());
    }
    let mut statuses = Vec::new();
    for service in services {
        let web = macos_networksetup_text(&["-getwebproxy".to_string(), service.clone()])?;
        let secure = macos_networksetup_text(&["-getsecurewebproxy".to_string(), service.clone()])?;
        let bypass =
            macos_networksetup_text(&["-getproxybypassdomains".to_string(), service.clone()])
                .unwrap_or_else(|err| format!("error: {err}"));
        if has_flag(args, "--json") {
            statuses.push(serde_json::json!({
                "service": service,
                "http": proxy_status_json(&web),
                "https": proxy_status_json(&secure),
                "bypass": bypass_domains_json(&bypass),
            }));
        } else {
            println!("service={service}");
            println!("  http  {}", compact_proxy_status(&web));
            println!("  https {}", compact_proxy_status(&secure));
            println!("  bypass {}", compact_bypass_domains(&bypass));
        }
    }
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "macos",
                "backend": "networksetup",
                "services": statuses,
            })
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn proxy_status_json(text: &str) -> serde_json::Value {
    serde_json::json!({
        "enabled": proxy_status_value(text, "Enabled").as_deref() == Some("Yes"),
        "server": proxy_status_value(text, "Server").filter(|value| !value.is_empty()),
        "port": proxy_status_value(text, "Port").and_then(|value| value.parse::<u16>().ok()),
        "authenticated": proxy_status_value(text, "Authenticated Proxy Enabled").as_deref() == Some("1"),
    })
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn bypass_domains_json(text: &str) -> Vec<String> {
    let compact = compact_bypass_domains(text);
    if compact == "-" {
        Vec::new()
    } else {
        compact.split(',').map(str::to_string).collect()
    }
}

#[cfg(not(target_os = "macos"))]
pub(in crate::cli) fn macos_system_proxy_status(_args: &[String]) -> Result<(), String> {
    Err(
        "system proxy management is only implemented for macOS networksetup in this build"
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn macos_system_proxy_set(args: &[String], enabled: bool) -> Result<(), String> {
    let services = system_proxy_services(args, false)?;
    let (host, port_num) = proxy_target(args)?;
    let dry_run = has_flag(args, "--dry-run");
    let bypass_domains = proxy_bypass_domains(args);

    let mut planned = Vec::new();
    for service in services {
        let commands = if enabled {
            let mut commands = vec![
                vec![
                    "-setwebproxy".to_string(),
                    service.clone(),
                    host.clone(),
                    port_num.to_string(),
                    "off".to_string(),
                    String::new(),
                    String::new(),
                ],
                vec![
                    "-setsecurewebproxy".to_string(),
                    service.clone(),
                    host.clone(),
                    port_num.to_string(),
                    "off".to_string(),
                    String::new(),
                    String::new(),
                ],
            ];
            if let Some(domains) = &bypass_domains {
                let mut bypass = vec!["-setproxybypassdomains".to_string(), service.clone()];
                if domains.is_empty() {
                    bypass.push("Empty".to_string());
                } else {
                    bypass.extend(domains.iter().cloned());
                }
                commands.push(bypass);
            }
            commands
        } else {
            vec![
                vec![
                    "-setwebproxystate".to_string(),
                    service.clone(),
                    "off".to_string(),
                ],
                vec![
                    "-setsecurewebproxystate".to_string(),
                    service.clone(),
                    "off".to_string(),
                ],
            ]
        };

        for command in commands {
            if dry_run {
                let line = format!(
                    "dry-run macos networksetup {}",
                    display_command_args(&command)
                );
                if has_flag(args, "--json") {
                    planned.push(line);
                } else {
                    println!("{line}");
                }
            } else {
                macos_networksetup_text(&command)?;
            }
        }
        if !has_flag(args, "--json") {
            println!(
                "proxy_{} service={} host={} port={}",
                if enabled { "on" } else { "off" },
                service,
                host,
                port_num
            );
        }
    }
    if dry_run && has_flag(args, "--json") {
        print_proxy_plan("macos", &planned, args);
    } else if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": "macos",
                "backend": "networksetup",
                "enabled": enabled,
                "host": host,
                "port": port_num,
            })
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(in crate::cli) fn macos_system_proxy_set(
    _args: &[String],
    _enabled: bool,
) -> Result<(), String> {
    Err(
        "system proxy management is only implemented for macOS networksetup in this build"
            .to_string(),
    )
}
