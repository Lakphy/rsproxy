use super::*;

mod linux;
mod macos;
mod macos_network;
mod windows;

pub(super) use linux::*;
pub(super) use macos::*;
#[cfg(target_os = "macos")]
pub(super) use macos_network::*;
pub(super) use windows::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProxyPlatform {
    Macos,
    Windows,
    Linux,
}

pub(super) fn system_proxy_cmd(args: Vec<String>) -> Result<(), String> {
    let sub = args.first().map(String::as_str).unwrap_or("status");
    match sub {
        "status" => system_proxy_status(&args),
        "on" => system_proxy_set(&args, true),
        "off" => system_proxy_set(&args, false),
        _ => Err(format!("unknown proxy command `{sub}`")),
    }
}

pub(super) fn system_proxy_status(args: &[String]) -> Result<(), String> {
    match proxy_platform(args)? {
        ProxyPlatform::Macos => macos_system_proxy_status(args),
        ProxyPlatform::Windows => windows_system_proxy_status(args),
        ProxyPlatform::Linux => linux_system_proxy_status(args),
    }
}

pub(super) fn system_proxy_set(args: &[String], enabled: bool) -> Result<(), String> {
    match proxy_platform(args)? {
        ProxyPlatform::Macos => macos_system_proxy_set(args, enabled),
        ProxyPlatform::Windows => windows_system_proxy_set(args, enabled),
        ProxyPlatform::Linux => linux_system_proxy_set(args, enabled),
    }
}

pub(super) fn proxy_platform(args: &[String]) -> Result<ProxyPlatform, String> {
    let platform = option_value(args, "--platform").unwrap_or_else(default_proxy_platform);
    match platform.to_ascii_lowercase().as_str() {
        "macos" | "darwin" => Ok(ProxyPlatform::Macos),
        "windows" | "win" => Ok(ProxyPlatform::Windows),
        "linux" => Ok(ProxyPlatform::Linux),
        _ => Err(format!("unsupported proxy platform `{platform}`")),
    }
}

pub(super) fn default_proxy_platform() -> String {
    if cfg!(target_os = "macos") {
        "macos".to_string()
    } else if cfg!(target_os = "windows") {
        "windows".to_string()
    } else {
        "linux".to_string()
    }
}

pub(super) fn print_proxy_plan(platform: &str, lines: &[String], args: &[String]) {
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "platform": platform,
                "dry_run": true,
                "commands": lines,
            })
        );
    } else {
        for line in lines {
            println!("{line}");
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn platform_command_output(
    program: &str,
    args: &[String],
) -> Result<std::process::Output, String> {
    let label = format!("{program} {}", display_command_args(args));
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{label}: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("{label}: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("{label}: {error}"))?;
            if output.status.success() {
                return Ok(output);
            }
            return Err(format!(
                "{label} failed: {}",
                platform_output_message(&output)
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|error| format!("{label}: {error}"))?;
            return Err(format!(
                "{label} timed out: {}",
                platform_output_message(&output)
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_output_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        output.status.to_string()
    }
}

pub(super) fn proxy_target(args: &[String]) -> Result<(String, u16), String> {
    let config = runtime_config(args)?;
    Ok((config.host, config.port))
}

pub(super) fn proxy_bypass_domains(args: &[String]) -> Option<Vec<String>> {
    option_value(args, "--bypass").map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    })
}

pub(super) fn display_command_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() {
                "\"\"".to_string()
            } else if arg.chars().any(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
