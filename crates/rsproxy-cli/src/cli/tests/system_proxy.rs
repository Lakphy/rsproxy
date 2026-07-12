use super::super::*;

#[test]
fn proxy_platform_parses_explicit_aliases() {
    assert_eq!(
        proxy_platform(&[
            "status".to_string(),
            "--platform".to_string(),
            "darwin".to_string()
        ])
        .unwrap(),
        ProxyPlatform::Macos
    );
    assert_eq!(
        proxy_platform(&[
            "status".to_string(),
            "--platform".to_string(),
            "win".to_string()
        ])
        .unwrap(),
        ProxyPlatform::Windows
    );
    assert_eq!(
        proxy_platform(&[
            "status".to_string(),
            "--platform".to_string(),
            "linux".to_string()
        ])
        .unwrap(),
        ProxyPlatform::Linux
    );
    assert!(
        proxy_platform(&[
            "status".to_string(),
            "--platform".to_string(),
            "freebsd".to_string(),
        ])
        .is_err()
    );
}

#[test]
fn windows_proxy_dry_run_renders_registry_plan() {
    let bypass = vec!["localhost".to_string(), "*.local".to_string()];
    let lines = windows_proxy_set_dry_run_lines(true, "127.0.0.1", 18916, Some(&bypass));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/v ProxyEnable /t REG_DWORD /d 1 /f"))
    );
    assert!(lines.iter().any(|line| {
        line.contains("/v ProxyServer /t REG_SZ /d http=127.0.0.1:18916;https=127.0.0.1:18916 /f")
    }));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/v ProxyOverride /t REG_SZ /d localhost;*.local /f"))
    );

    let lines = windows_proxy_set_dry_run_lines(false, "127.0.0.1", 18916, None);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/v ProxyEnable /t REG_DWORD /d 0 /f"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("reg delete") && line.contains("/v ProxyServer /f"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("reg delete") && line.contains("/v ProxyOverride /f"))
    );
}

#[test]
fn linux_proxy_dry_run_renders_gsettings_and_env_plan() {
    let bypass = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let lines = linux_proxy_set_dry_run_lines(true, "127.0.0.1", 18916, Some(&bypass));
    assert!(
        lines.contains(
            &"dry-run linux gsettings set org.gnome.system.proxy mode manual".to_string()
        )
    );
    assert!(lines.contains(
        &"dry-run linux gsettings set org.gnome.system.proxy.http host 127.0.0.1".to_string()
    ));
    assert!(lines.contains(
        &"dry-run linux gsettings set org.gnome.system.proxy.http port 18916".to_string()
    ));
    assert!(lines.contains(&"dry-run linux gsettings set org.gnome.system.proxy ignore-hosts \"['localhost', '127.0.0.1']\"".to_string()));
    assert!(lines.contains(&"dry-run linux env export http_proxy=http://127.0.0.1:18916 https_proxy=http://127.0.0.1:18916 all_proxy=http://127.0.0.1:18916".to_string()));

    let lines = linux_proxy_set_dry_run_lines(false, "127.0.0.1", 18916, None);
    assert_eq!(
        lines[0],
        "dry-run linux gsettings set org.gnome.system.proxy mode none"
    );
    assert_eq!(
        lines[1],
        "dry-run linux env unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY"
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
    let args = vec!["--config".to_string(), path.display().to_string()];
    assert_eq!(proxy_target(&args).unwrap(), ("0.0.0.0".to_string(), 18888));

    let args = vec![
        "--config".to_string(),
        path.display().to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        "28888".to_string(),
    ];
    assert_eq!(
        proxy_target(&args).unwrap(),
        ("127.0.0.1".to_string(), 28888)
    );
    let _ = fs::remove_file(path);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_network_service_parser_filters_headers_disabled_markers_and_blanks() {
    let output = "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*USB 10/100/1000 LAN\n\nThunderbolt Bridge\n";
    assert_eq!(
        parse_macos_network_services(output).unwrap(),
        ["Wi-Fi", "USB 10/100/1000 LAN", "Thunderbolt Bridge"]
    );
    assert!(parse_macos_network_services("\nAn asterisk marks disabled services\n").is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_proxy_status_and_bypass_parsers_cover_missing_and_invalid_fields() {
    let status = "Enabled: Yes\nServer: 127.0.0.1\nPort: 18916\nAuthenticated Proxy Enabled: 1\n";
    assert_eq!(
        proxy_status_value(status, "enabled").as_deref(),
        Some("Yes")
    );
    assert_eq!(proxy_status_value("malformed", "Enabled"), None);
    assert_eq!(
        compact_proxy_status(status),
        "enabled=Yes server=127.0.0.1 port=18916 authenticated=1"
    );
    let json = proxy_status_json(status);
    assert_eq!(json["enabled"], true);
    assert_eq!(json["server"], "127.0.0.1");
    assert_eq!(json["port"], 18916);
    assert_eq!(json["authenticated"], true);

    let missing = proxy_status_json("Enabled: No\nServer:\nPort: invalid\n");
    assert_eq!(missing["enabled"], false);
    assert!(missing["server"].is_null());
    assert!(missing["port"].is_null());
    assert_eq!(
        compact_proxy_status("Enabled: No\nServer:\n"),
        "enabled=No server=- port=- authenticated=-"
    );

    assert_eq!(
        compact_bypass_domains("localhost\n*.local\n"),
        "localhost,*.local"
    );
    assert_eq!(compact_bypass_domains("\n"), "-");
    assert_eq!(
        compact_bypass_domains("There aren't any bypass domains"),
        "-"
    );
    assert_eq!(
        bypass_domains_json("localhost\n*.local"),
        ["localhost", "*.local"]
    );
    assert!(bypass_domains_json("There aren't any").is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_service_selection_and_dry_run_cover_command_variants() {
    let explicit = vec!["--service".to_string(), "Wi-Fi".to_string()];
    assert_eq!(system_proxy_services(&explicit, false).unwrap(), ["Wi-Fi"]);
    assert!(system_proxy_services(&[], false).is_err());

    let enabled = vec![
        "on".to_string(),
        "--platform".to_string(),
        "macos".to_string(),
        "--service".to_string(),
        "Wi-Fi".to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        "18916".to_string(),
        "--bypass".to_string(),
        "localhost, *.local".to_string(),
        "--dry-run".to_string(),
    ];
    macos_system_proxy_set(&enabled, true).unwrap();

    let empty_bypass = vec![
        "on".to_string(),
        "--service".to_string(),
        "USB LAN".to_string(),
        "--bypass".to_string(),
        ", ,".to_string(),
        "--dry-run".to_string(),
        "--json".to_string(),
    ];
    macos_system_proxy_set(&empty_bypass, true).unwrap();

    let disabled = vec![
        "off".to_string(),
        "--service".to_string(),
        "Wi-Fi".to_string(),
        "--dry-run".to_string(),
    ];
    macos_system_proxy_set(&disabled, false).unwrap();

    let status = vec![
        "status".to_string(),
        "--service".to_string(),
        "Wi-Fi".to_string(),
        "--dry-run".to_string(),
    ];
    macos_system_proxy_status(&status).unwrap();
}
