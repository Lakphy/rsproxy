#[cfg(target_os = "macos")]
use super::*;

#[cfg(target_os = "macos")]
pub(in crate::cli) fn system_proxy_services(
    args: &[String],
    default_all: bool,
) -> Result<Vec<String>, String> {
    if let Some(service) = option_value(args, "--service") {
        return Ok(vec![service]);
    }
    if has_flag(args, "--all") || default_all {
        return macos_network_services();
    }
    Err(
        "proxy on/off requires --service NAME or --all; use --dry-run to preview commands"
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn macos_network_services() -> Result<Vec<String>, String> {
    let output = macos_networksetup_text(&["-listallnetworkservices".to_string()])?;
    parse_macos_network_services(&output)
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn parse_macos_network_services(output: &str) -> Result<Vec<String>, String> {
    let services = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.contains("An asterisk"))
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix('*').unwrap_or(line).trim().to_string())
        .collect::<Vec<_>>();
    if services.is_empty() {
        return Err("networksetup did not report any network services".to_string());
    }
    Ok(services)
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn macos_networksetup_text(args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new("networksetup");
    cmd.args(args);
    let label = format!("networksetup {}", display_command_args(args));
    let output = security_raw_output(&label, &mut cmd)?;
    if !output.status.success() {
        return Err(format!(
            "{label} failed: {}",
            security_output_message(&output)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn compact_proxy_status(text: &str) -> String {
    let enabled = proxy_status_value(text, "Enabled").unwrap_or_else(|| "-".to_string());
    let server = proxy_status_value(text, "Server")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string());
    let port = proxy_status_value(text, "Port").unwrap_or_else(|| "-".to_string());
    let auth =
        proxy_status_value(text, "Authenticated Proxy Enabled").unwrap_or_else(|| "-".to_string());
    format!("enabled={enabled} server={server} port={port} authenticated={auth}")
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn proxy_status_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (raw_key, value) = line.split_once(':')?;
        if raw_key.trim().eq_ignore_ascii_case(key) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn compact_bypass_domains(text: &str) -> String {
    let domains = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if domains.is_empty() || domains.iter().any(|line| line.starts_with("There aren't")) {
        "-".to_string()
    } else {
        domains.join(",")
    }
}
