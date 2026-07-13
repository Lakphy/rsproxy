use super::*;

#[test]
fn upstream_route_parses_http_proxy_chain() {
    let request = meta("http://origin.test:18080/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(proxy://127.0.0.1:18001, proxy://127.0.0.1:18002)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::ProxyChain {
            hops: vec![
                ProxyHop::Http {
                    host: "127.0.0.1".to_string(),
                    port: 18001
                },
                ProxyHop::Http {
                    host: "127.0.0.1".to_string(),
                    port: 18002
                }
            ],
            target_host: "origin.test".to_string(),
            target_port: 18080
        }
    );
    assert_eq!(
        route.session_label(),
        "proxy://127.0.0.1:18001->proxy://127.0.0.1:18002"
    );
    assert_eq!(
        route.tunnel_session_label(),
        "proxy://127.0.0.1:18001->proxy://127.0.0.1:18002->origin.test:18080"
    );
    assert!(route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_http_to_socks_proxy_chain() {
    let request = meta("http://origin.test:18080/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(proxy://127.0.0.1:18001, socks5://127.0.0.1:18002)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::ProxyChain {
            hops: vec![
                ProxyHop::Http {
                    host: "127.0.0.1".to_string(),
                    port: 18001
                },
                ProxyHop::Socks5 {
                    host: "127.0.0.1".to_string(),
                    port: 18002,
                    auth: None
                }
            ],
            target_host: "origin.test".to_string(),
            target_port: 18080
        }
    );
    assert_eq!(
        route.session_label(),
        "proxy://127.0.0.1:18001->socks5://127.0.0.1:18002"
    );
    assert_eq!(
        route.tunnel_session_label(),
        "proxy://127.0.0.1:18001->socks5://127.0.0.1:18002->origin.test:18080"
    );
    assert!(!route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_socks_to_http_proxy_chain_with_auth_redaction() {
    let request = meta("http://origin.test/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(socks5://alice:secret@127.0.0.1:18001, proxy://127.0.0.1:18002)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route.session_label(),
        "socks5://auth@127.0.0.1:18001->proxy://127.0.0.1:18002"
    );
    assert!(route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_http_to_https_proxy_chain() {
    let request = meta("http://origin.test:18080/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(proxy://127.0.0.1:18001, https-proxy://secure-proxy.test:18443)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::ProxyChain {
            hops: vec![
                ProxyHop::Http {
                    host: "127.0.0.1".to_string(),
                    port: 18001
                },
                ProxyHop::Https {
                    host: "secure-proxy.test".to_string(),
                    port: 18443
                }
            ],
            target_host: "origin.test".to_string(),
            target_port: 18080
        }
    );
    assert_eq!(
        route.session_label(),
        "proxy://127.0.0.1:18001->https-proxy://secure-proxy.test:18443"
    );
    assert_eq!(
        route.tunnel_session_label(),
        "proxy://127.0.0.1:18001->https-proxy://secure-proxy.test:18443->origin.test:18080"
    );
    assert!(route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_https_to_socks_proxy_chain() {
    let request = meta("http://origin.test/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(https-proxy://secure-proxy.test:18443, socks5://127.0.0.1:18002)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route.session_label(),
        "https-proxy://secure-proxy.test:18443->socks5://127.0.0.1:18002"
    );
    assert_eq!(
        route.tunnel_session_label(),
        "https-proxy://secure-proxy.test:18443->socks5://127.0.0.1:18002->origin.test:80"
    );
    assert!(!route.uses_absolute_form());
}

#[test]
fn upstream_route_parses_nested_https_proxy_chain() {
    let request = meta("http://origin.test/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test upstream(https-proxy://p1.test:18443, https-proxy://p2.test:19443)",
    )
    .unwrap();
    let resolved = rules.resolve(&request);
    let url = UrlParts::parse(&request.url).unwrap();
    let route = test_upstream_route(&url, &resolved.actions, &request);

    assert_eq!(
        route,
        UpstreamRoute::ProxyChain {
            hops: vec![
                ProxyHop::Https {
                    host: "p1.test".to_string(),
                    port: 18443
                },
                ProxyHop::Https {
                    host: "p2.test".to_string(),
                    port: 19443
                }
            ],
            target_host: "origin.test".to_string(),
            target_port: 80
        }
    );
    assert_eq!(
        route.session_label(),
        "https-proxy://p1.test:18443->https-proxy://p2.test:19443"
    );
    assert_eq!(
        route.tunnel_session_label(),
        "https-proxy://p1.test:18443->https-proxy://p2.test:19443->origin.test:80"
    );
    assert!(route.uses_absolute_form());
}
