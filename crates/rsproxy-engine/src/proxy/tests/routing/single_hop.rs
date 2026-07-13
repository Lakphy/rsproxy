use super::*;

#[test]
fn upstream_route_parses_socks5_proxy_and_target() {
    let request = meta("http://origin.test:18080/path");
    let rules =
        rsproxy_rules::RuleSet::parse("default", "origin.test upstream(socks5://127.0.0.1:1081)")
            .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::Socks5 {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 1081,
            auth: None,
            target_host: "origin.test".to_string(),
            target_port: 18080
        }
    );
    assert_eq!(
        route.session_label(),
        "socks5://127.0.0.1:1081->origin.test:18080"
    );
    assert!(!route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_socks5_auth_without_leaking_password() {
    let request = meta("http://origin.test/path");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test upstream(socks5://alice:secret@127.0.0.1:1081)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::Socks5 {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 1081,
            auth: Some(SocksAuth {
                username: "alice".to_string(),
                password: "secret".to_string()
            }),
            target_host: "origin.test".to_string(),
            target_port: 80
        }
    );
    assert_eq!(
        route.session_label(),
        "socks5://auth@127.0.0.1:1081->origin.test:80"
    );
}

#[test]
fn upstream_route_parses_http_proxy_tunnel_target() {
    let request = meta("tunnel://target.test:443");
    let rules =
        rsproxy_rules::RuleSet::parse("default", "target.test upstream(proxy://127.0.0.1:18888)")
            .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::HttpProxy {
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 18888,
            target_host: "target.test".to_string(),
            target_port: 443
        }
    );
    assert_eq!(
        route.tunnel_session_label(),
        "proxy://127.0.0.1:18888->target.test:443"
    );
}

#[test]
fn direct_route_overrides_matched_upstream_actions() {
    let request = meta("http://origin.test:18080/direct");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test upstream(proxy://127.0.0.1:18888)\norigin.test/direct direct",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert!(
        resolved
            .actions
            .iter()
            .any(|item| matches!(item.action, Action::Upstream(_)))
    );
    assert!(
        resolved
            .actions
            .iter()
            .any(|item| matches!(item.action, Action::Direct))
    );
    assert_eq!(
        route,
        UpstreamRoute::Direct {
            host: "origin.test".to_string(),
            port: 18080
        }
    );
    assert!(!route.uses_absolute_form());

    let same_line = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test direct upstream(proxy://127.0.0.1:18888)",
    )
    .unwrap();
    let resolved = same_line.resolve(&request);
    let route = test_upstream_route(&url, &resolved.actions, &request);
    assert_eq!(
        route,
        UpstreamRoute::Direct {
            host: "origin.test".to_string(),
            port: 18080
        }
    );
}

#[test]
fn upstream_route_parses_https_proxy() {
    let request = meta("http://origin.test/path");
    let rules = rsproxy_rules::RuleSet::parse(
        "default",
        "origin.test upstream(https-proxy://secure-proxy.test:18443)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::HttpsProxy {
            proxy_host: "secure-proxy.test".to_string(),
            proxy_port: 18443,
            target_host: "origin.test".to_string(),
            target_port: 80
        }
    );
    assert_eq!(route.connect_addr(), "secure-proxy.test:18443");
    assert_eq!(
        route.session_label(),
        "https-proxy://secure-proxy.test:18443"
    );
    assert!(route.uses_absolute_form());
    assert!(route.uses_tls_to_proxy());
}
