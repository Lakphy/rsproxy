use super::command::{
    ClientArgs, ProxyArgs, ProxyCommand as CliProxyCommand, ProxyMutationArgs, ProxyPlatformArg,
    RuntimeArgs,
};
use super::config::runtime_config;
use crate::{CliError, CliResult};
use rsproxy_platform::system_proxy::{
    LinuxSettingStatus, MacosBypassStatus, MacosEndpointStatus, MacosServiceStatus, ProxyAction,
    ProxyChange, ProxyCommand, ProxyOptions, ProxyOutcome, ProxyPlan, ProxyPlanStep, ProxyStatus,
    ProxyTarget, execute_system_proxy, plan_system_proxy,
};

pub(super) use rsproxy_platform::system_proxy::ProxyPlatform;

pub(super) enum SystemProxyResult {
    Plan(ProxyPlan),
    Outcome(ProxyOutcome),
}

pub(super) fn system_proxy_cmd(args: ProxyArgs, json: bool) -> CliResult<()> {
    let (action, mutation) = match args.command {
        None | Some(CliProxyCommand::Status(_)) => (ProxyAction::Status, None),
        Some(CliProxyCommand::On(args)) => (ProxyAction::Enable, Some(args)),
        Some(CliProxyCommand::Off(args)) => (ProxyAction::Disable, Some(args)),
    };
    let platform = proxy_platform(args.platform);
    let options = proxy_options(&args.client, args.service, mutation, action)?;
    let result = if args.dry_run {
        SystemProxyResult::Plan(plan_system_proxy(platform, action, &options)?)
    } else {
        SystemProxyResult::Outcome(execute_system_proxy(platform, action, &options)?)
    };
    render_proxy_report(result, json)
}

pub(super) fn proxy_platform(platform: Option<ProxyPlatformArg>) -> ProxyPlatform {
    match platform {
        Some(ProxyPlatformArg::Macos) => ProxyPlatform::Macos,
        Some(ProxyPlatformArg::Windows) => ProxyPlatform::Windows,
        Some(ProxyPlatformArg::Linux) => ProxyPlatform::Linux,
        None => default_proxy_platform(),
    }
}

fn default_proxy_platform() -> ProxyPlatform {
    if cfg!(target_os = "macos") {
        ProxyPlatform::Macos
    } else if cfg!(target_os = "windows") {
        ProxyPlatform::Windows
    } else {
        ProxyPlatform::Linux
    }
}

pub(super) fn proxy_options(
    client: &ClientArgs,
    service: Option<String>,
    mutation: Option<ProxyMutationArgs>,
    action: ProxyAction,
) -> CliResult<ProxyOptions> {
    let target = if action == ProxyAction::Status {
        None
    } else {
        let mutation = mutation.as_ref().ok_or(CliError::InvalidPlatformOutcome {
            detail: "system proxy mutation options are missing",
        })?;
        let (host, port) = proxy_target(client, mutation)?;
        Some(ProxyTarget { host, port })
    };
    Ok(ProxyOptions {
        target,
        bypass: mutation.as_ref().and_then(proxy_bypass_domains),
        service,
        all_services: mutation.as_ref().is_some_and(|args| args.all),
    })
}

fn render_proxy_report(report: SystemProxyResult, json: bool) -> CliResult<()> {
    if json {
        println!("{}", proxy_report_json(&report)?);
    } else {
        for line in proxy_report_lines(&report) {
            println!("{line}");
        }
    }
    Ok(())
}

pub(super) fn proxy_report_json(report: &SystemProxyResult) -> CliResult<serde_json::Value> {
    match report {
        SystemProxyResult::Plan(plan) => Ok(serde_json::json!({
            "platform": platform_name(plan.platform),
            "dry_run": true,
            "commands": plan
                .steps
                .iter()
                .filter_map(|step| match step {
                    ProxyPlanStep::Command(command) => Some(proxy_command_line(command)),
                    ProxyPlanStep::Change(_) => None,
                })
                .collect::<Vec<_>>(),
        })),
        SystemProxyResult::Outcome(ProxyOutcome::Status(status)) => Ok(proxy_status_json(status)),
        SystemProxyResult::Outcome(ProxyOutcome::Changed(changes)) => changes
            .first()
            .map(proxy_change_json)
            .ok_or(CliError::InvalidPlatformOutcome {
                detail: "system proxy mutation returned no changes",
            }),
    }
}

pub(super) fn proxy_report_lines(report: &SystemProxyResult) -> Vec<String> {
    match report {
        SystemProxyResult::Plan(plan) => proxy_plan_lines(plan),
        SystemProxyResult::Outcome(ProxyOutcome::Status(status)) => proxy_status_lines(status),
        SystemProxyResult::Outcome(ProxyOutcome::Changed(changes)) => {
            changes.iter().map(proxy_change_line).collect()
        }
    }
}

fn proxy_plan_lines(plan: &ProxyPlan) -> Vec<String> {
    plan.steps
        .iter()
        .map(|step| match step {
            ProxyPlanStep::Command(command) => proxy_command_line(command),
            ProxyPlanStep::Change(change) => proxy_change_line(change),
        })
        .collect()
}

fn proxy_command_line(command: &ProxyCommand) -> String {
    let (platform, program, args) = match command {
        ProxyCommand::MacosNetworkSetup { args } => ("macos", "networksetup", args),
        ProxyCommand::WindowsRegistry { args } => ("windows", "reg", args),
        ProxyCommand::LinuxGsettings { args } => ("linux", "gsettings", args),
        ProxyCommand::LinuxEnvironment { args } => ("linux", "env", args),
    };
    format!(
        "dry-run {platform} {program} {}",
        display_command_args(args)
    )
}

fn proxy_change_line(change: &ProxyChange) -> String {
    let action = if change.enabled { "on" } else { "off" };
    if let Some(service) = &change.service {
        format!(
            "proxy_{action} service={} host={} port={}",
            service, change.target.host, change.target.port
        )
    } else {
        format!(
            "proxy_{action} platform={} host={} port={}",
            platform_name(change.platform),
            change.target.host,
            change.target.port
        )
    }
}

fn proxy_change_json(change: &ProxyChange) -> serde_json::Value {
    match change.platform {
        ProxyPlatform::Macos => serde_json::json!({
            "platform": "macos",
            "backend": "networksetup",
            "enabled": change.enabled,
            "host": change.target.host,
            "port": change.target.port,
        }),
        ProxyPlatform::Windows => serde_json::json!({
            "platform": "windows",
            "backend": "wininet-registry",
            "enabled": change.enabled,
            "host": change.target.host,
            "port": change.target.port,
            "bypass": change.bypass,
        }),
        ProxyPlatform::Linux => serde_json::json!({
            "platform": "linux",
            "backend": "gsettings",
            "enabled": change.enabled,
            "host": change.target.host,
            "port": change.target.port,
            "bypass": change.bypass,
        }),
    }
}

fn proxy_status_json(status: &ProxyStatus) -> serde_json::Value {
    match status {
        ProxyStatus::Macos { services } => serde_json::json!({
            "platform": "macos",
            "backend": "networksetup",
            "services": services
                .iter()
                .map(macos_service_json)
                .collect::<Vec<_>>(),
        }),
        ProxyStatus::Windows {
            enabled,
            server,
            bypass,
        } => serde_json::json!({
            "platform": "windows",
            "backend": "wininet-registry",
            "enabled": enabled,
            "server": server,
            "bypass": bypass,
        }),
        ProxyStatus::Linux { settings } => serde_json::json!({
            "platform": "linux",
            "backend": "gsettings",
            "settings": settings
                .iter()
                .map(linux_setting_json)
                .collect::<Vec<_>>(),
        }),
    }
}

fn linux_setting_json(setting: &LinuxSettingStatus) -> serde_json::Value {
    serde_json::json!({
        "schema": setting.schema,
        "key": setting.key,
        "value": setting.value,
    })
}

fn macos_service_json(status: &MacosServiceStatus) -> serde_json::Value {
    serde_json::json!({
        "service": status.service,
        "http": macos_endpoint_json(&status.http),
        "https": macos_endpoint_json(&status.https),
        "bypass": macos_bypass_json(&status.bypass),
    })
}

fn macos_endpoint_json(status: &MacosEndpointStatus) -> serde_json::Value {
    serde_json::json!({
        "enabled": status.enabled,
        "server": status.server,
        "port": status.port,
        "authenticated": status.authenticated,
    })
}

fn macos_bypass_json(status: &MacosBypassStatus) -> Vec<String> {
    match status {
        MacosBypassStatus::Domains(domains) => domains.clone(),
        MacosBypassStatus::QueryError(error) => format!("error: {error}")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(",")
            .split(',')
            .map(str::to_string)
            .collect(),
    }
}

fn proxy_status_lines(status: &ProxyStatus) -> Vec<String> {
    match status {
        ProxyStatus::Macos { services } => services
            .iter()
            .flat_map(macos_service_lines)
            .collect::<Vec<_>>(),
        ProxyStatus::Windows {
            enabled,
            server,
            bypass,
        } => vec![
            format!("enabled={enabled}"),
            format!("server={}", server.as_deref().unwrap_or("-")),
            format!("bypass={}", bypass.as_deref().unwrap_or("-")),
        ],
        ProxyStatus::Linux { settings } => settings
            .iter()
            .map(|setting| format!("{}.{}={}", setting.schema, setting.key, setting.value))
            .collect(),
    }
}

fn macos_service_lines(status: &MacosServiceStatus) -> Vec<String> {
    vec![
        format!("service={}", status.service),
        format!("  http  {}", macos_endpoint_line(&status.http)),
        format!("  https {}", macos_endpoint_line(&status.https)),
        format!("  bypass {}", macos_bypass_line(&status.bypass)),
    ]
}

fn macos_endpoint_line(status: &MacosEndpointStatus) -> String {
    format!(
        "enabled={} server={} port={} authenticated={}",
        status.reported_enabled.as_deref().unwrap_or("-"),
        status.server.as_deref().unwrap_or("-"),
        status.reported_port.as_deref().unwrap_or("-"),
        status.reported_authenticated.as_deref().unwrap_or("-"),
    )
}

fn macos_bypass_line(status: &MacosBypassStatus) -> String {
    match status {
        MacosBypassStatus::Domains(domains) if domains.is_empty() => "-".to_string(),
        MacosBypassStatus::Domains(domains) => domains.join(","),
        MacosBypassStatus::QueryError(error) => format!("error: {error}")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn platform_name(platform: ProxyPlatform) -> &'static str {
    match platform {
        ProxyPlatform::Macos => "macos",
        ProxyPlatform::Windows => "windows",
        ProxyPlatform::Linux => "linux",
    }
}

fn display_command_args(args: &[String]) -> String {
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

pub(super) fn proxy_target(
    client: &ClientArgs,
    mutation: &ProxyMutationArgs,
) -> CliResult<(String, u16)> {
    let mut runtime = RuntimeArgs::from_client(client.clone());
    runtime.host.clone_from(&mutation.host);
    runtime.port = mutation.port;
    let config = runtime_config(&runtime)?;
    Ok((config.host, config.port))
}

fn proxy_bypass_domains(args: &ProxyMutationArgs) -> Option<Vec<String>> {
    args.bypass.as_ref().map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    })
}
