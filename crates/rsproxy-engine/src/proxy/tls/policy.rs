use super::*;

pub(in crate::proxy) fn connect_bypass(actions: &[ResolvedAction]) -> bool {
    actions
        .iter()
        .any(|item| matches!(item.action, Action::Bypass))
}

pub(in crate::proxy) fn upstream_mtls_enabled(
    full_url: &str,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> bool {
    let Some(item) = tls_action(actions) else {
        return false;
    };
    if tls_action_op(item).client_cert.is_none() {
        return false;
    }
    let Ok(url) = UrlParts::parse(full_url) else {
        return false;
    };
    let Ok(route) = upstream_route(&url, actions, meta, state) else {
        return false;
    };
    origin_tls_supported(&url, &route)
}

pub(in crate::proxy) fn origin_tls_supported(url: &UrlParts, route: &UpstreamRoute) -> bool {
    url.scheme == "https"
        && (matches!(
            route,
            UpstreamRoute::Direct { .. } | UpstreamRoute::Socks5 { .. }
        ) || route.uses_proxy_tunnel_for_https_origin(url))
}

pub(in crate::proxy) fn apply_upstream_tls_policy_flags(
    session: &mut Session,
    full_url: &str,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) {
    let Some(item) = tls_action(actions) else {
        return;
    };
    let op = tls_action_op(item);
    if op.min_version.is_none() && op.ciphers.is_empty() {
        return;
    }
    let Ok(url) = UrlParts::parse(full_url) else {
        return;
    };
    let Ok(route) = upstream_route(&url, actions, meta, state) else {
        return;
    };
    if !origin_tls_supported(&url, &route) {
        return;
    }
    session.flags.push("upstream-tls-policy".to_string());
    if let Some(min_version) = op.min_version {
        session
            .flags
            .push(format!("upstream-tls-min:{}", min_version.as_str()));
    }
    if !op.ciphers.is_empty() {
        session
            .flags
            .push(format!("upstream-tls-ciphers:{}", op.ciphers.len()));
    }
}

pub(in crate::proxy) fn tls_action(actions: &[ResolvedAction]) -> Option<&ResolvedAction> {
    actions
        .iter()
        .find(|item| matches!(item.action, Action::Tls(_)))
}

pub(in crate::proxy) fn tls_action_op(item: &ResolvedAction) -> &TlsOp {
    match &item.action {
        Action::Tls(op) => op,
        _ => unreachable!("tls_action only returns TLS actions"),
    }
}
