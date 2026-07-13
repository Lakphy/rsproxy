use super::*;

pub(super) fn services(options: &ProxyOptions, default_all: bool) -> PlatformResult<Vec<String>> {
    if let Some(service) = &options.service {
        return Ok(vec![service.clone()]);
    }
    if options.all_services || default_all {
        return network_services();
    }
    Err(PlatformError::InvalidState(
        "proxy on/off requires --service NAME or --all; use --dry-run to preview commands"
            .to_string(),
    ))
}

#[cfg(target_os = "macos")]
fn network_services() -> PlatformResult<Vec<String>> {
    let output = networksetup_text(&["-listallnetworkservices".to_string()])?;
    parse_network_services(&output)
}

#[cfg(not(target_os = "macos"))]
fn network_services() -> PlatformResult<Vec<String>> {
    Err(PlatformError::Unsupported(
        "system proxy management is only implemented for macOS networksetup in this build"
            .to_string(),
    ))
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_network_services(output: &str) -> PlatformResult<Vec<String>> {
    let services = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.contains("An asterisk"))
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix('*').unwrap_or(line).trim().to_string())
        .collect::<Vec<_>>();
    if services.is_empty() {
        return Err(PlatformError::InvalidState(
            "networksetup did not report any network services".to_string(),
        ));
    }
    Ok(services)
}

#[cfg(target_os = "macos")]
pub(super) fn networksetup_text(args: &[String]) -> PlatformResult<String> {
    let label = format!("networksetup {}", display_command_args(args));
    let output = command_output(
        &label,
        "networksetup",
        args,
        Duration::from_secs(15),
        Some("macOS may be waiting for an authentication dialog"),
    )?;
    if !output.status.success() {
        return Err(PlatformError::CommandFailed {
            command: label,
            status: output.status.code(),
            output: platform_output_message(&output),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn networksetup_text(_args: &[String]) -> PlatformResult<String> {
    Err(PlatformError::Unsupported(
        "system proxy management is only implemented for macOS networksetup in this build"
            .to_string(),
    ))
}

pub(super) fn proxy_status_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (raw_key, value) = line.split_once(':')?;
        if raw_key.trim().eq_ignore_ascii_case(key) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}
