use super::*;

mod model;

pub(super) use model::{ProxyHop, SocksAuth, UpstreamRoute};

pub(super) fn upstream_route(
    url: &UrlParts,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<UpstreamRoute> {
    let (target_host, target_port) = upstream_target(url, actions, meta, state)?;
    if actions
        .iter()
        .any(|item| matches!(item.action, Action::Direct))
    {
        return Ok(UpstreamRoute::Direct {
            host: target_host,
            port: target_port,
        });
    }
    for item in actions {
        if let Action::Upstream(value) = &item.action {
            let rendered = resolve_value_text(value, item, meta, state)?;
            let entries = upstream_entries(&rendered);
            if entries.len() > 1 {
                let hops = entries
                    .iter()
                    .filter_map(|entry| parse_proxy_hop(entry))
                    .collect::<Vec<_>>();
                if hops.len() == entries.len() {
                    return Ok(UpstreamRoute::ProxyChain {
                        hops,
                        target_host,
                        target_port,
                    });
                }
            }
            let value = entries.first().copied().unwrap_or(rendered.as_str());
            if let Some(addr) = value
                .strip_prefix("proxy://")
                .or_else(|| value.strip_prefix("http://"))
            {
                let (proxy_host, proxy_port) = split_addr(addr, 80);
                return Ok(UpstreamRoute::HttpProxy {
                    proxy_host,
                    proxy_port,
                    target_host,
                    target_port,
                });
            }
            if let Some(addr) = value.strip_prefix("https-proxy://") {
                let (proxy_host, proxy_port) = split_addr(addr, 443);
                return Ok(UpstreamRoute::HttpsProxy {
                    proxy_host,
                    proxy_port,
                    target_host,
                    target_port,
                });
            }
            if let Some(addr) = value
                .strip_prefix("socks://")
                .or_else(|| value.strip_prefix("socks5://"))
            {
                let (auth, addr) = split_socks_auth(addr);
                let (proxy_host, proxy_port) = split_addr(&addr, 1080);
                return Ok(UpstreamRoute::Socks5 {
                    proxy_host,
                    proxy_port,
                    auth,
                    target_host,
                    target_port,
                });
            }
        }
    }
    Ok(UpstreamRoute::Direct {
        host: target_host,
        port: target_port,
    })
}

pub(super) fn upstream_pool_key(
    url: &UrlParts,
    route: &UpstreamRoute,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> String {
    let tls_policy = tls_action(actions)
        .map(|item| {
            let op = tls_action_op(item);
            format!(
                "min={:?};ciphers={:?};cert={};key={}",
                op.min_version,
                op.ciphers,
                op.client_cert
                    .as_deref()
                    .map(|value| item.render(value, meta))
                    .unwrap_or_default(),
                op.client_key
                    .as_deref()
                    .map(|value| item.render(value, meta))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_default();
    format!(
        "state={};storage={};origin={}:{};route={route:?};tls={tls_policy};headers={}:{}",
        state.rules.identity(),
        state.config.storage.display(),
        url.host,
        url.effective_port().unwrap_or(443),
        state.config.max_header_size,
        state.config.max_header_count,
    )
}

pub(super) fn upstream_entries(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

pub(super) fn parse_proxy_hop(value: &str) -> Option<ProxyHop> {
    if let Some(addr) = value
        .strip_prefix("proxy://")
        .or_else(|| value.strip_prefix("http://"))
    {
        let (host, port) = split_addr(addr, 80);
        return Some(ProxyHop::Http { host, port });
    }
    if let Some(addr) = value.strip_prefix("https-proxy://") {
        let (host, port) = split_addr(addr, 443);
        return Some(ProxyHop::Https { host, port });
    }
    if let Some(addr) = value
        .strip_prefix("socks://")
        .or_else(|| value.strip_prefix("socks5://"))
    {
        let (auth, addr) = split_socks_auth(addr);
        let (host, port) = split_addr(&addr, 1080);
        return Some(ProxyHop::Socks5 { host, port, auth });
    }
    None
}

pub(super) fn upstream_target(
    url: &UrlParts,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<(String, u16)> {
    for item in actions {
        if let Action::Host(pool) = &item.action {
            let rendered = resolve_value_text(pool.selected_address(), item, meta, state)?;
            let (host, port) = split_addr(&rendered, url.effective_port().unwrap_or(80));
            return Ok((host, port));
        }
    }
    Ok((url.host.clone(), url.effective_port().unwrap_or(80)))
}

pub(super) fn planned_upstream_addr(
    full_url: &str,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> Option<String> {
    let url = UrlParts::parse(full_url).ok()?;
    Some(
        upstream_route(&url, actions, meta, state)
            .ok()?
            .session_label(),
    )
}

pub(super) fn literal_ip_from_url(url: &str) -> Option<String> {
    let host = UrlParts::parse(url).ok()?.host;
    host.parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}
