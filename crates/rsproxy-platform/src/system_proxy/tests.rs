use super::*;

fn target() -> ProxyTarget {
    ProxyTarget {
        host: "127.0.0.1".to_string(),
        port: 18916,
    }
}

fn command_args(command: &ProxyCommand) -> &[String] {
    match command {
        ProxyCommand::MacosNetworkSetup { args }
        | ProxyCommand::WindowsRegistry { args }
        | ProxyCommand::LinuxGsettings { args }
        | ProxyCommand::LinuxEnvironment { args } => args,
    }
}

#[test]
fn windows_plan_exposes_registry_operations_as_typed_commands() {
    let bypass = vec!["localhost".to_string(), "*.local".to_string()];
    let commands = windows::proxy_set_plan_commands(true, "127.0.0.1", 18916, Some(&bypass));
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, ProxyCommand::WindowsRegistry { .. }))
    );
    assert!(commands.iter().any(|command| {
        command_args(command)
            == [
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                "ProxyEnable",
                "/t",
                "REG_DWORD",
                "/d",
                "1",
                "/f",
            ]
    }));
    assert!(commands.iter().any(|command| {
        command_args(command)
            .iter()
            .any(|arg| arg == "http=127.0.0.1:18916;https=127.0.0.1:18916")
    }));

    let commands = windows::proxy_set_plan_commands(false, "127.0.0.1", 18916, None);
    assert!(commands.iter().any(|command| {
        command_args(command)
            == [
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                "ProxyServer",
                "/f",
            ]
    }));
}

#[test]
fn linux_plan_separates_gsettings_and_environment_operations() {
    let bypass = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let commands = linux::proxy_set_plan_commands(true, "127.0.0.1", 18916, Some(&bypass));
    assert!(commands.iter().any(|command| matches!(
        command,
        ProxyCommand::LinuxGsettings { args }
            if args == &["set", "org.gnome.system.proxy", "mode", "manual"]
    )));
    assert!(commands.iter().any(|command| matches!(
        command,
        ProxyCommand::LinuxGsettings { args }
            if args.last().is_some_and(|arg| arg == "['localhost', '127.0.0.1']")
    )));
    assert!(commands.iter().any(|command| matches!(
        command,
        ProxyCommand::LinuxEnvironment { args }
            if args == &["export", "http_proxy=http://127.0.0.1:18916", "https_proxy=http://127.0.0.1:18916", "all_proxy=http://127.0.0.1:18916"]
    )));
}

#[test]
fn macos_parser_returns_service_and_field_values_without_rendering() {
    let output = "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*USB 10/100/1000 LAN\n\nThunderbolt Bridge\n";
    assert_eq!(
        macos_network::parse_network_services(output).unwrap(),
        ["Wi-Fi", "USB 10/100/1000 LAN", "Thunderbolt Bridge"]
    );
    assert!(matches!(
        macos_network::parse_network_services("\nAn asterisk marks disabled services\n"),
        Err(PlatformError::InvalidState(_))
    ));
    let status = "Enabled: Yes\nServer: 127.0.0.1\nPort: 18916\nAuthenticated Proxy Enabled: 1\n";
    assert_eq!(
        macos_network::proxy_status_value(status, "enabled").as_deref(),
        Some("Yes")
    );
    assert_eq!(
        macos_network::proxy_status_value(status, "Port").as_deref(),
        Some("18916")
    );
}

#[test]
fn mutation_without_a_target_is_a_typed_state_error() {
    assert!(matches!(
        plan_system_proxy(
            ProxyPlatform::Windows,
            ProxyAction::Enable,
            &ProxyOptions::default(),
        ),
        Err(PlatformError::InvalidState(_))
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn command_timeout_retains_operation_budget_and_safe_hint() {
    let args = vec!["-c".to_string(), "sleep 1".to_string()];
    let error = command_output(
        "slow platform command",
        "/bin/sh",
        &args,
        Duration::from_millis(10),
        Some("authentication may be pending"),
    )
    .unwrap_err();

    match error {
        PlatformError::Timeout {
            operation,
            timeout_ms,
            output,
        } => {
            assert_eq!(operation, "slow platform command");
            assert_eq!(timeout_ms, 10);
            assert!(output.contains("authentication may be pending"));
        }
        other => panic!("expected typed timeout, got {other:?}"),
    }
}

#[test]
fn typed_dispatch_returns_plan_steps_for_every_platform() {
    for platform in [
        ProxyPlatform::Macos,
        ProxyPlatform::Windows,
        ProxyPlatform::Linux,
    ] {
        let options = ProxyOptions {
            target: Some(target()),
            bypass: Some(vec!["localhost".to_string(), "*.local".to_string()]),
            service: (platform == ProxyPlatform::Macos).then(|| "Contract Service".to_string()),
            all_services: false,
        };
        let plan = plan_system_proxy(platform, ProxyAction::Enable, &options).unwrap();
        assert_eq!(plan.platform, platform);
        assert!(
            plan.steps
                .iter()
                .any(|step| matches!(step, ProxyPlanStep::Command(_)))
        );
        assert!(
            plan.steps
                .iter()
                .any(|step| matches!(step, ProxyPlanStep::Change(_)))
        );
    }
}
