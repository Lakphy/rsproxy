use super::*;

pub(super) fn connect_upstream_stream(
    ctx: &ForwardCtx<'_>,
    allow_origin_h2: bool,
    tls_records: &mut Vec<TlsRecord>,
    network_timings: &mut NetworkTimings,
) -> io::Result<UpstreamStream> {
    let url = ctx.url;
    let route = ctx.route;
    let actions = ctx.actions;
    let meta = ctx.meta;
    let state = ctx.state;
    let deadline = ctx.deadline;
    if route.uses_proxy_tunnel_for_https_origin(url) {
        let upstream =
            connect_tunnel_upstream_recorded(route, state, tls_records, network_timings, deadline)?;
        let client_identity = tls_client_identity(actions, meta, state)?;
        let tls_policy = tls_action(actions).map(tls_action_op);
        return tls_wrap_upstream_stream(
            upstream,
            TlsWrapInput {
                tls_host: &url.host,
                client_identity,
                tls_policy,
                allow_h2: allow_origin_h2,
                state,
                deadline,
            },
            tls_records,
        );
    }

    if let UpstreamRoute::ProxyChain {
        hops,
        target_host,
        target_port,
    } = route
    {
        let mut upstream =
            connect_proxy_chain_to_final(hops, state, tls_records, network_timings, deadline)?;
        if let Some(ProxyHop::Socks5 { auth, .. }) = hops.last() {
            socks5_connect_with_deadline(
                &mut upstream,
                target_host,
                *target_port,
                auth.as_ref(),
                deadline,
            )?;
        }
        restore_upstream_timeouts(&mut upstream)?;
        return Ok(upstream);
    }

    let upstream_addr = route.connect_addr();
    let tcp = connect_tcp_with_timeouts(&upstream_addr, state, network_timings, deadline)?;
    let mut upstream = UpstreamStream::Tcp(tcp);
    if let UpstreamRoute::Socks5 {
        target_host,
        target_port,
        auth,
        ..
    } = route
    {
        socks5_connect_with_deadline(
            &mut upstream,
            target_host,
            *target_port,
            auth.as_ref(),
            deadline,
        )?;
    }
    let origin_tls = origin_tls_supported(url, route);
    let client_identity = if origin_tls {
        tls_client_identity(actions, meta, state)?
    } else {
        None
    };
    if route.uses_tls_to_proxy() {
        tls_wrap_upstream_stream(
            upstream,
            TlsWrapInput {
                tls_host: route.tls_host(),
                client_identity: None,
                tls_policy: None,
                allow_h2: false,
                state,
                deadline,
            },
            tls_records,
        )
    } else if origin_tls {
        tls_wrap_upstream_stream(
            upstream,
            TlsWrapInput {
                tls_host: &url.host,
                client_identity,
                tls_policy: tls_action(actions).map(tls_action_op),
                allow_h2: allow_origin_h2,
                state,
                deadline,
            },
            tls_records,
        )
    } else {
        restore_upstream_timeouts(&mut upstream)?;
        Ok(upstream)
    }
}

pub(super) fn connect_tunnel_upstream(
    route: &UpstreamRoute,
    state: &SharedState,
    network_timings: &mut NetworkTimings,
    deadline: RequestDeadline,
) -> io::Result<UpstreamStream> {
    let mut tls_records = Vec::new();
    connect_tunnel_upstream_recorded(route, state, &mut tls_records, network_timings, deadline)
}

pub(super) fn connect_tunnel_upstream_recorded(
    route: &UpstreamRoute,
    state: &SharedState,
    tls_records: &mut Vec<TlsRecord>,
    network_timings: &mut NetworkTimings,
    deadline: RequestDeadline,
) -> io::Result<UpstreamStream> {
    match route {
        UpstreamRoute::Direct { .. } => {
            connect_tcp_with_timeouts(&route.connect_addr(), state, network_timings, deadline)
                .map(UpstreamStream::Tcp)
        }
        UpstreamRoute::Socks5 {
            target_host,
            target_port,
            auth,
            ..
        } => {
            let tcp =
                connect_tcp_with_timeouts(&route.connect_addr(), state, network_timings, deadline)?;
            let mut upstream = UpstreamStream::Tcp(tcp);
            socks5_connect_with_deadline(
                &mut upstream,
                target_host,
                *target_port,
                auth.as_ref(),
                deadline,
            )?;
            restore_upstream_timeouts(&mut upstream)?;
            Ok(upstream)
        }
        UpstreamRoute::HttpProxy { .. } => {
            let tcp =
                connect_tcp_with_timeouts(&route.connect_addr(), state, network_timings, deadline)?;
            let mut upstream = UpstreamStream::Tcp(tcp);
            http_proxy_connect_with_deadline(
                &mut upstream,
                &route.tunnel_target_addr(),
                state.config.max_header_size,
                state.config.max_header_count,
                deadline,
            )?;
            restore_upstream_timeouts(&mut upstream)?;
            Ok(upstream)
        }
        UpstreamRoute::ProxyChain { hops, .. } => {
            let mut upstream =
                connect_proxy_chain_to_final(hops, state, tls_records, network_timings, deadline)?;
            match hops.last() {
                Some(ProxyHop::Http { .. } | ProxyHop::Https { .. }) => {
                    http_proxy_connect_with_deadline(
                        &mut upstream,
                        &route.tunnel_target_addr(),
                        state.config.max_header_size,
                        state.config.max_header_count,
                        deadline,
                    )?
                }
                Some(ProxyHop::Socks5 { auth, .. }) => {
                    let (host, port) = route.tunnel_target_parts();
                    socks5_connect_with_deadline(
                        &mut upstream,
                        host,
                        port,
                        auth.as_ref(),
                        deadline,
                    )?;
                }
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "proxy chain is empty",
                    ));
                }
            }
            restore_upstream_timeouts(&mut upstream)?;
            Ok(upstream)
        }
        UpstreamRoute::HttpsProxy { .. } => {
            let tcp =
                connect_tcp_with_timeouts(&route.connect_addr(), state, network_timings, deadline)?;
            let mut tls = tls_wrap_upstream_stream(
                UpstreamStream::Tcp(tcp),
                TlsWrapInput {
                    tls_host: route.tls_host(),
                    client_identity: None,
                    tls_policy: None,
                    allow_h2: false,
                    state,
                    deadline,
                },
                tls_records,
            )?;
            http_proxy_connect_with_deadline(
                &mut tls,
                &route.tunnel_target_addr(),
                state.config.max_header_size,
                state.config.max_header_count,
                deadline,
            )?;
            restore_upstream_timeouts(&mut tls)?;
            Ok(tls)
        }
    }
}

pub(super) fn connect_proxy_chain_to_final(
    hops: &[ProxyHop],
    state: &SharedState,
    tls_records: &mut Vec<TlsRecord>,
    network_timings: &mut NetworkTimings,
    deadline: RequestDeadline,
) -> io::Result<UpstreamStream> {
    let Some(first) = hops.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "proxy chain is empty",
        ));
    };
    let tcp = connect_tcp_with_timeouts(&first.addr(), state, network_timings, deadline)?;
    let mut upstream = UpstreamStream::Tcp(tcp);
    if let ProxyHop::Https { host, .. } = first {
        upstream = tls_wrap_upstream_stream(
            upstream,
            TlsWrapInput {
                tls_host: host,
                client_identity: None,
                tls_policy: None,
                allow_h2: false,
                state,
                deadline,
            },
            tls_records,
        )?;
    }
    for idx in 1..hops.len() {
        let prev = &hops[idx - 1];
        let next = &hops[idx];
        match prev {
            ProxyHop::Http { .. } | ProxyHop::Https { .. } => {
                http_proxy_connect_with_deadline(
                    &mut upstream,
                    &next.addr(),
                    state.config.max_header_size,
                    state.config.max_header_count,
                    deadline,
                )?;
            }
            ProxyHop::Socks5 { auth, .. } => {
                socks5_connect_with_deadline(
                    &mut upstream,
                    next.host(),
                    next.port(),
                    auth.as_ref(),
                    deadline,
                )?;
            }
        }
        if let ProxyHop::Https { host, .. } = next {
            upstream = tls_wrap_upstream_stream(
                upstream,
                TlsWrapInput {
                    tls_host: host,
                    client_identity: None,
                    tls_policy: None,
                    allow_h2: false,
                    state,
                    deadline,
                },
                tls_records,
            )?;
        }
    }
    restore_upstream_timeouts(&mut upstream)?;
    Ok(upstream)
}

fn http_proxy_connect_with_deadline(
    stream: &mut UpstreamStream,
    target: &str,
    max_header_size: usize,
    max_header_count: usize,
    deadline: RequestDeadline,
) -> io::Result<()> {
    let mut io = DeadlineIo::new(stream, deadline);
    http_proxy_connect_tunnel(&mut io, target, max_header_size, max_header_count)
        .map_err(|error| stage_io_error("proxy_connect", error))
}

fn socks5_connect_with_deadline(
    stream: &mut UpstreamStream,
    target_host: &str,
    target_port: u16,
    auth: Option<&SocksAuth>,
    deadline: RequestDeadline,
) -> io::Result<()> {
    let mut io = DeadlineIo::new(stream, deadline);
    socks5_connect(&mut io, target_host, target_port, auth)
        .map_err(|error| stage_io_error("socks5", error))
}
