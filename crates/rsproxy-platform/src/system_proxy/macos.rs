use super::macos_network::{networksetup_text, proxy_status_value, services};
use super::*;

pub(super) fn plan(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyPlan> {
    match action.enabled() {
        None => status_plan(options),
        Some(enabled) => set_plan(options, enabled),
    }
}

pub(super) fn execute(action: ProxyAction, options: &ProxyOptions) -> PlatformResult<ProxyOutcome> {
    match action.enabled() {
        None => native_status(services(options, true)?),
        Some(enabled) => {
            let services = services(options, false)?;
            let target = required_target(options)?;
            let commands = services
                .iter()
                .map(|service| {
                    (
                        service.clone(),
                        mutation_commands(service, enabled, target, options.bypass.as_deref()),
                    )
                })
                .collect();
            native_set(commands, enabled, target, options.bypass.as_deref())
        }
    }
}

fn status_plan(options: &ProxyOptions) -> PlatformResult<ProxyPlan> {
    let services = services(options, true)?;
    let steps = services
        .into_iter()
        .flat_map(|service| {
            [
                vec!["-getwebproxy".to_string(), service.clone()],
                vec!["-getsecurewebproxy".to_string(), service.clone()],
                vec!["-getproxybypassdomains".to_string(), service],
            ]
            .into_iter()
            .map(|args| ProxyPlanStep::Command(ProxyCommand::MacosNetworkSetup { args }))
        })
        .collect();
    Ok(ProxyPlan::new(ProxyPlatform::Macos, steps))
}

fn set_plan(options: &ProxyOptions, enabled: bool) -> PlatformResult<ProxyPlan> {
    let services = services(options, false)?;
    let target = required_target(options)?;
    let steps = services
        .into_iter()
        .flat_map(|service| {
            let mut steps = mutation_commands(&service, enabled, target, options.bypass.as_deref())
                .into_iter()
                .map(|args| ProxyPlanStep::Command(ProxyCommand::MacosNetworkSetup { args }))
                .collect::<Vec<_>>();
            steps.push(ProxyPlanStep::Change(ProxyChange {
                platform: ProxyPlatform::Macos,
                enabled,
                target: target.clone(),
                bypass: options.bypass.clone(),
                service: Some(service),
            }));
            steps
        })
        .collect();
    Ok(ProxyPlan::new(ProxyPlatform::Macos, steps))
}

fn mutation_commands(
    service: &str,
    enabled: bool,
    target: &ProxyTarget,
    bypass_domains: Option<&[String]>,
) -> Vec<Vec<String>> {
    if !enabled {
        return vec![
            vec![
                "-setwebproxystate".to_string(),
                service.to_string(),
                "off".to_string(),
            ],
            vec![
                "-setsecurewebproxystate".to_string(),
                service.to_string(),
                "off".to_string(),
            ],
        ];
    }

    let mut commands = vec![
        vec![
            "-setwebproxy".to_string(),
            service.to_string(),
            target.host.clone(),
            target.port.to_string(),
            "off".to_string(),
            String::new(),
            String::new(),
        ],
        vec![
            "-setsecurewebproxy".to_string(),
            service.to_string(),
            target.host.clone(),
            target.port.to_string(),
            "off".to_string(),
            String::new(),
            String::new(),
        ],
    ];
    if let Some(domains) = bypass_domains {
        let mut bypass = vec!["-setproxybypassdomains".to_string(), service.to_string()];
        if domains.is_empty() {
            bypass.push("Empty".to_string());
        } else {
            bypass.extend(domains.iter().cloned());
        }
        commands.push(bypass);
    }
    commands
}

fn endpoint_status(text: &str) -> MacosEndpointStatus {
    let reported_enabled = proxy_status_value(text, "Enabled");
    let server = proxy_status_value(text, "Server").filter(|value| !value.is_empty());
    let reported_port = proxy_status_value(text, "Port");
    let reported_authenticated = proxy_status_value(text, "Authenticated Proxy Enabled");
    MacosEndpointStatus {
        enabled: reported_enabled.as_deref() == Some("Yes"),
        server,
        port: reported_port
            .as_deref()
            .and_then(|value| value.parse::<u16>().ok()),
        authenticated: reported_authenticated.as_deref() == Some("1"),
        reported_enabled,
        reported_port,
        reported_authenticated,
    }
}

fn bypass_status(text: &str) -> MacosBypassStatus {
    let domains = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if domains.is_empty() || domains.iter().any(|line| line.starts_with("There aren't")) {
        MacosBypassStatus::Domains(Vec::new())
    } else {
        MacosBypassStatus::Domains(domains.into_iter().map(str::to_string).collect())
    }
}

fn native_status(services: Vec<String>) -> PlatformResult<ProxyOutcome> {
    if !cfg!(target_os = "macos") {
        return Err(PlatformError::Unsupported(
            "system proxy management is only implemented for macOS networksetup in this build"
                .to_string(),
        ));
    }
    let mut statuses = Vec::new();
    for service in services {
        let web = networksetup_text(&["-getwebproxy".to_string(), service.clone()])?;
        let secure = networksetup_text(&["-getsecurewebproxy".to_string(), service.clone()])?;
        let bypass =
            match networksetup_text(&["-getproxybypassdomains".to_string(), service.clone()]) {
                Ok(text) => bypass_status(&text),
                Err(error) => MacosBypassStatus::QueryError(error.to_string()),
            };
        statuses.push(MacosServiceStatus {
            service,
            http: endpoint_status(&web),
            https: endpoint_status(&secure),
            bypass,
        });
    }
    Ok(ProxyOutcome::Status(ProxyStatus::Macos {
        services: statuses,
    }))
}

fn native_set(
    services: Vec<(String, Vec<Vec<String>>)>,
    enabled: bool,
    target: &ProxyTarget,
    bypass: Option<&[String]>,
) -> PlatformResult<ProxyOutcome> {
    if !cfg!(target_os = "macos") {
        return Err(PlatformError::Unsupported(
            "system proxy management is only implemented for macOS networksetup in this build"
                .to_string(),
        ));
    }
    let mut changes = Vec::new();
    for (service, commands) in services {
        for command in commands {
            networksetup_text(&command)?;
        }
        changes.push(ProxyChange {
            platform: ProxyPlatform::Macos,
            enabled,
            target: target.clone(),
            bypass: bypass.map(<[String]>::to_vec),
            service: Some(service),
        });
    }
    Ok(ProxyOutcome::Changed(changes))
}
