use crate::cli::command::{Cli, ClientArgs, ProxyMutationArgs, ProxyPlatformArg, TopLevelCommand};
use crate::cli::system_proxy::{
    SystemProxyResult, proxy_options, proxy_platform, proxy_report_json, proxy_report_lines,
    proxy_target,
};
use clap::Parser;
use rsproxy_platform::system_proxy::{
    LinuxSettingStatus, MacosBypassStatus, MacosEndpointStatus, MacosServiceStatus, ProxyAction,
    ProxyChange, ProxyCommand, ProxyOutcome, ProxyPlan, ProxyPlanStep, ProxyPlatform, ProxyStatus,
    ProxyTarget,
};
use std::fs;

#[test]
fn proxy_platform_parses_explicit_aliases() {
    for (value, expected) in [
        ("darwin", ProxyPlatform::Macos),
        ("win", ProxyPlatform::Windows),
        ("linux", ProxyPlatform::Linux),
    ] {
        let cli = Cli::try_parse_from(["rsproxy", "proxy", "status", "--platform", value]).unwrap();
        let Some(TopLevelCommand::Proxy(args)) = cli.command else {
            panic!("proxy command expected");
        };
        assert_eq!(proxy_platform(args.platform), expected);
    }
    assert!(Cli::try_parse_from(["rsproxy", "proxy", "status", "--platform", "freebsd"]).is_err());
    assert_eq!(
        proxy_platform(Some(ProxyPlatformArg::Windows)),
        ProxyPlatform::Windows
    );
}

#[test]
fn proxy_target_uses_config_and_cli_precedence() {
    let path = std::env::temp_dir().join(format!(
        "rsproxy-proxy-config-{}-{}.toml",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&path, "host = \"0.0.0.0\"\nport = 18888\n").unwrap();
    let client = ClientArgs {
        config: Some(path.clone()),
        ..ClientArgs::default()
    };
    let defaults = ProxyMutationArgs {
        host: None,
        port: None,
        bypass: None,
        all: false,
    };
    assert_eq!(
        proxy_target(&client, &defaults).unwrap(),
        ("0.0.0.0".to_string(), 18888)
    );
    let overrides = ProxyMutationArgs {
        host: Some("127.0.0.1".to_string()),
        port: Some(28888),
        bypass: None,
        all: false,
    };
    assert_eq!(
        proxy_target(&client, &overrides).unwrap(),
        ("127.0.0.1".to_string(), 28888)
    );
    let _ = fs::remove_file(path);
}

#[test]
fn proxy_options_parse_cli_selection_and_bypass_without_platform_argv_leaks() {
    let mutation = ProxyMutationArgs {
        host: Some("127.0.0.1".to_string()),
        port: Some(18888),
        bypass: Some("localhost, *.local, ,".to_string()),
        all: true,
    };
    let options = proxy_options(
        &ClientArgs::default(),
        Some("Wi-Fi".to_string()),
        Some(mutation),
        ProxyAction::Enable,
    )
    .unwrap();
    assert_eq!(options.target.unwrap().host, "127.0.0.1");
    assert_eq!(options.service.as_deref(), Some("Wi-Fi"));
    assert!(options.all_services);
    assert_eq!(options.bypass.unwrap(), ["localhost", "*.local"]);

    let status = proxy_options(&ClientArgs::default(), None, None, ProxyAction::Status).unwrap();
    assert!(status.target.is_none());
}

#[test]
fn cli_renders_typed_plan_into_the_existing_human_and_json_contracts() {
    let report = SystemProxyResult::Plan(ProxyPlan {
        platform: ProxyPlatform::Windows,
        steps: vec![
            ProxyPlanStep::Command(ProxyCommand::WindowsRegistry {
                args: vec![
                    "add".to_string(),
                    r"HKCU\Software\Internet Settings".to_string(),
                    "/v".to_string(),
                    "ProxyEnable".to_string(),
                    "/d".to_string(),
                    "1".to_string(),
                ],
            }),
            ProxyPlanStep::Change(ProxyChange {
                platform: ProxyPlatform::Windows,
                enabled: true,
                target: ProxyTarget {
                    host: "127.0.0.1".to_string(),
                    port: 18916,
                },
                bypass: None,
                service: None,
            }),
        ],
    });
    assert_eq!(
        proxy_report_lines(&report),
        [
            r#"dry-run windows reg add "HKCU\\Software\\Internet Settings" /v ProxyEnable /d 1"#,
            "proxy_on platform=windows host=127.0.0.1 port=18916",
        ]
    );
    let json = proxy_report_json(&report).unwrap();
    assert_eq!(json["platform"], "windows");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["commands"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_renders_typed_macos_status_without_platform_presentation_fields() {
    let endpoint = MacosEndpointStatus {
        enabled: true,
        server: Some("127.0.0.1".to_string()),
        port: Some(18916),
        authenticated: true,
        reported_enabled: Some("Yes".to_string()),
        reported_port: Some("18916".to_string()),
        reported_authenticated: Some("1".to_string()),
    };
    let report = SystemProxyResult::Outcome(ProxyOutcome::Status(ProxyStatus::Macos {
        services: vec![MacosServiceStatus {
            service: "Wi-Fi".to_string(),
            http: endpoint.clone(),
            https: endpoint,
            bypass: MacosBypassStatus::Domains(vec!["localhost".to_string()]),
        }],
    }));
    assert_eq!(
        proxy_report_lines(&report),
        [
            "service=Wi-Fi",
            "  http  enabled=Yes server=127.0.0.1 port=18916 authenticated=1",
            "  https enabled=Yes server=127.0.0.1 port=18916 authenticated=1",
            "  bypass localhost",
        ]
    );
    let json = proxy_report_json(&report).unwrap();
    assert_eq!(json["platform"], "macos");
    assert_eq!(json["services"][0]["http"]["enabled"], true);
    assert_eq!(json["services"][0]["bypass"][0], "localhost");
}

#[test]
fn cli_renders_changed_outcomes_for_every_platform_and_rejects_empty_results() {
    for (platform, backend) in [
        (ProxyPlatform::Macos, "networksetup"),
        (ProxyPlatform::Windows, "wininet-registry"),
        (ProxyPlatform::Linux, "gsettings"),
    ] {
        let report = SystemProxyResult::Outcome(ProxyOutcome::Changed(vec![proxy_change(
            platform,
            false,
            (platform == ProxyPlatform::Macos).then(|| "Wi-Fi".to_string()),
        )]));
        let lines = proxy_report_lines(&report);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("proxy_off"));
        let json = proxy_report_json(&report).unwrap();
        assert_eq!(json["backend"], backend);
        assert_eq!(json["enabled"], false);
    }

    let empty = SystemProxyResult::Outcome(ProxyOutcome::Changed(Vec::new()));
    assert!(proxy_report_json(&empty).is_err());
    assert!(proxy_report_lines(&empty).is_empty());
}

#[test]
fn cli_renders_windows_linux_and_macos_error_status_shapes() {
    let windows = SystemProxyResult::Outcome(ProxyOutcome::Status(ProxyStatus::Windows {
        enabled: false,
        server: None,
        bypass: None,
    }));
    assert_eq!(
        proxy_report_lines(&windows),
        ["enabled=false", "server=-", "bypass=-"]
    );
    let windows_json = proxy_report_json(&windows).unwrap();
    assert_eq!(windows_json["platform"], "windows");
    assert!(windows_json["server"].is_null());

    let linux = SystemProxyResult::Outcome(ProxyOutcome::Status(ProxyStatus::Linux {
        settings: vec![LinuxSettingStatus {
            schema: "org.gnome.system.proxy".to_string(),
            key: "mode".to_string(),
            value: "'manual'".to_string(),
        }],
    }));
    assert_eq!(
        proxy_report_lines(&linux),
        ["org.gnome.system.proxy.mode='manual'"]
    );
    let linux_json = proxy_report_json(&linux).unwrap();
    assert_eq!(linux_json["settings"][0]["key"], "mode");

    let endpoint = MacosEndpointStatus {
        enabled: false,
        server: None,
        port: None,
        authenticated: false,
        reported_enabled: None,
        reported_port: None,
        reported_authenticated: None,
    };
    let macos = SystemProxyResult::Outcome(ProxyOutcome::Status(ProxyStatus::Macos {
        services: vec![MacosServiceStatus {
            service: "Ethernet".to_string(),
            http: endpoint.clone(),
            https: endpoint,
            bypass: MacosBypassStatus::QueryError(
                " first failure \n\n second failure ".to_string(),
            ),
        }],
    }));
    let lines = proxy_report_lines(&macos);
    assert_eq!(
        lines[1],
        "  http  enabled=- server=- port=- authenticated=-"
    );
    assert_eq!(lines[3], "  bypass error:  first failure,second failure");
    let macos_json = proxy_report_json(&macos).unwrap();
    assert_eq!(
        macos_json["services"][0]["bypass"],
        serde_json::json!(["error:  first failure", "second failure"])
    );

    let expected = if cfg!(target_os = "macos") {
        ProxyPlatform::Macos
    } else if cfg!(target_os = "windows") {
        ProxyPlatform::Windows
    } else {
        ProxyPlatform::Linux
    };
    assert_eq!(proxy_platform(None), expected);
    assert!(proxy_options(&ClientArgs::default(), None, None, ProxyAction::Enable).is_err());
}

fn proxy_change(platform: ProxyPlatform, enabled: bool, service: Option<String>) -> ProxyChange {
    ProxyChange {
        platform,
        enabled,
        target: ProxyTarget {
            host: "127.0.0.1".to_string(),
            port: 18916,
        },
        bypass: Some(vec!["localhost".to_string()]),
        service,
    }
}
