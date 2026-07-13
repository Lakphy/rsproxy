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

#[test]
fn typed_plans_cover_status_disable_and_empty_bypass_variants() {
    let macos_status = plan_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Status,
        &ProxyOptions {
            service: Some("Coverage Service".to_string()),
            ..ProxyOptions::default()
        },
    )
    .unwrap();
    assert_eq!(macos_status.steps.len(), 3);
    assert!(macos_status.steps.iter().all(|step| matches!(
        step,
        ProxyPlanStep::Command(ProxyCommand::MacosNetworkSetup { .. })
    )));

    let macos_disable = plan_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Disable,
        &ProxyOptions {
            target: Some(target()),
            service: Some("Coverage Service".to_string()),
            ..ProxyOptions::default()
        },
    )
    .unwrap();
    assert_eq!(macos_disable.steps.len(), 3);
    assert!(matches!(
        macos_disable.steps.last(),
        Some(ProxyPlanStep::Change(ProxyChange { enabled: false, .. }))
    ));

    let macos_empty_bypass = plan_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Enable,
        &ProxyOptions {
            target: Some(target()),
            bypass: Some(Vec::new()),
            service: Some("Coverage Service".to_string()),
            ..ProxyOptions::default()
        },
    )
    .unwrap();
    assert!(macos_empty_bypass.steps.iter().any(|step| matches!(
        step,
        ProxyPlanStep::Command(ProxyCommand::MacosNetworkSetup { args })
            if args == &["-setproxybypassdomains", "Coverage Service", "Empty"]
    )));

    for platform in [ProxyPlatform::Windows, ProxyPlatform::Linux] {
        let status =
            plan_system_proxy(platform, ProxyAction::Status, &ProxyOptions::default()).unwrap();
        assert!(!status.steps.is_empty());
    }

    let windows_empty_bypass = plan_system_proxy(
        ProxyPlatform::Windows,
        ProxyAction::Enable,
        &ProxyOptions {
            target: Some(target()),
            bypass: Some(Vec::new()),
            ..ProxyOptions::default()
        },
    )
    .unwrap();
    assert!(windows_empty_bypass.steps.iter().any(|step| matches!(
        step,
        ProxyPlanStep::Command(ProxyCommand::WindowsRegistry { args })
            if args.iter().any(|arg| arg == "<local>")
    )));

    let linux_empty_bypass = plan_system_proxy(
        ProxyPlatform::Linux,
        ProxyAction::Enable,
        &ProxyOptions {
            target: Some(target()),
            bypass: Some(Vec::new()),
            ..ProxyOptions::default()
        },
    )
    .unwrap();
    assert!(linux_empty_bypass.steps.iter().any(|step| matches!(
        step,
        ProxyPlanStep::Command(ProxyCommand::LinuxGsettings { args })
            if args.last().is_some_and(|arg| arg == "[]")
    )));

    let missing_scope = plan_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Disable,
        &ProxyOptions {
            target: Some(target()),
            ..ProxyOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(missing_scope, PlatformError::InvalidState(_)));
}

#[cfg(target_os = "macos")]
#[test]
fn native_dispatch_reports_unsupported_backends_without_mutation() {
    for (platform, action) in [
        (ProxyPlatform::Windows, ProxyAction::Status),
        (ProxyPlatform::Windows, ProxyAction::Enable),
        (ProxyPlatform::Linux, ProxyAction::Status),
        (ProxyPlatform::Linux, ProxyAction::Disable),
    ] {
        let error = execute_system_proxy(
            platform,
            action,
            &ProxyOptions {
                target: Some(target()),
                ..ProxyOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, PlatformError::Unsupported(_)));
    }

    let missing_scope = execute_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Enable,
        &ProxyOptions {
            target: Some(target()),
            ..ProxyOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(missing_scope, PlatformError::InvalidState(_)));

    let missing_target = execute_system_proxy(
        ProxyPlatform::Macos,
        ProxyAction::Enable,
        &ProxyOptions {
            service: Some("Coverage Service".to_string()),
            ..ProxyOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(missing_target, PlatformError::InvalidState(_)));
}

#[cfg(target_os = "macos")]
#[test]
fn command_helpers_quote_arguments_and_preserve_output_context() {
    assert_eq!(
        display_command_args(&["plain".to_string(), "two words".to_string(), String::new(),]),
        "plain \"two words\" \"\""
    );

    let args = vec![
        "-c".to_string(),
        "printf 'stdout'; printf 'stderr' >&2".to_string(),
    ];
    let output = command_output(
        "successful command",
        "/bin/sh",
        &args,
        Duration::from_secs(1),
        None,
    )
    .unwrap();
    assert_eq!(platform_output_message(&output), "stderr; stdout");

    for (script, expected) in [
        ("printf 'stderr' >&2", "stderr"),
        ("printf 'stdout'", "stdout"),
    ] {
        let output = Command::new("/bin/sh")
            .args(["-c", script])
            .output()
            .unwrap();
        assert_eq!(platform_output_message(&output), expected);
    }
    let empty = Command::new("/bin/sh")
        .args(["-c", "exit 3"])
        .output()
        .unwrap();
    assert!(platform_output_message(&empty).contains("exit status: 3"));

    let error = command_output(
        "missing command",
        "/path/that/does/not/exist/rsproxy-platform-coverage",
        &[],
        Duration::from_secs(1),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PlatformError::Io { context, source }
            if context == "missing command" && source.kind() == std::io::ErrorKind::NotFound
    ));

    let slow_args = vec!["-c".to_string(), "sleep 1".to_string()];
    let error = command_output(
        "unhinted timeout",
        "/bin/sh",
        &slow_args,
        Duration::from_millis(10),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PlatformError::Timeout {
            operation,
            timeout_ms: 10,
            ..
        } if operation == "unhinted timeout"
    ));
}
