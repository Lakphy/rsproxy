use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum UpstreamRoute {
    Direct {
        host: String,
        port: u16,
    },
    HttpProxy {
        proxy_host: String,
        proxy_port: u16,
        target_host: String,
        target_port: u16,
    },
    ProxyChain {
        hops: Vec<ProxyHop>,
        target_host: String,
        target_port: u16,
    },
    HttpsProxy {
        proxy_host: String,
        proxy_port: u16,
        target_host: String,
        target_port: u16,
    },
    Socks5 {
        proxy_host: String,
        proxy_port: u16,
        auth: Option<SocksAuth>,
        target_host: String,
        target_port: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum ProxyHop {
    Http {
        host: String,
        port: u16,
    },
    Https {
        host: String,
        port: u16,
    },
    Socks5 {
        host: String,
        port: u16,
        auth: Option<SocksAuth>,
    },
}

impl ProxyHop {
    pub(in crate::proxy) fn addr(&self) -> String {
        format_host_port(self.host(), self.port())
    }

    pub(in crate::proxy) fn label(&self) -> String {
        match self {
            ProxyHop::Http { host, port } => {
                format!("proxy://{}", format_host_port(host, *port))
            }
            ProxyHop::Https { host, port } => {
                format!("https-proxy://{}", format_host_port(host, *port))
            }
            ProxyHop::Socks5 { host, port, auth } => {
                let auth_prefix = if auth.is_some() { "auth@" } else { "" };
                format!("socks5://{auth_prefix}{}", format_host_port(host, *port))
            }
        }
    }

    pub(in crate::proxy) fn host(&self) -> &str {
        match self {
            ProxyHop::Http { host, .. }
            | ProxyHop::Https { host, .. }
            | ProxyHop::Socks5 { host, .. } => host,
        }
    }

    pub(in crate::proxy) fn port(&self) -> u16 {
        match self {
            ProxyHop::Http { port, .. }
            | ProxyHop::Https { port, .. }
            | ProxyHop::Socks5 { port, .. } => *port,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::proxy) struct SocksAuth {
    pub(in crate::proxy) username: String,
    pub(in crate::proxy) password: String,
}

impl UpstreamRoute {
    pub(in crate::proxy) fn connect_addr(&self) -> String {
        match self {
            UpstreamRoute::Direct { host, port } => format_host_port(host, *port),
            UpstreamRoute::HttpProxy {
                proxy_host,
                proxy_port,
                ..
            } => format_host_port(proxy_host, *proxy_port),
            UpstreamRoute::ProxyChain { hops, .. } => hops[0].addr(),
            UpstreamRoute::HttpsProxy {
                proxy_host,
                proxy_port,
                ..
            }
            | UpstreamRoute::Socks5 {
                proxy_host,
                proxy_port,
                ..
            } => format_host_port(proxy_host, *proxy_port),
        }
    }

    pub(in crate::proxy) fn session_label(&self) -> String {
        match self {
            UpstreamRoute::Direct { host, port } => format_host_port(host, *port),
            UpstreamRoute::HttpProxy {
                proxy_host,
                proxy_port,
                ..
            } => format!("proxy://{}", format_host_port(proxy_host, *proxy_port)),
            UpstreamRoute::ProxyChain { hops, .. } => hops
                .iter()
                .map(ProxyHop::label)
                .collect::<Vec<_>>()
                .join("->"),
            UpstreamRoute::HttpsProxy {
                proxy_host,
                proxy_port,
                ..
            } => format!(
                "https-proxy://{}",
                format_host_port(proxy_host, *proxy_port)
            ),
            UpstreamRoute::Socks5 {
                proxy_host,
                proxy_port,
                auth,
                target_host,
                target_port,
            } => {
                let auth_prefix = if auth.is_some() { "auth@" } else { "" };
                format!(
                    "socks5://{auth_prefix}{}->{}",
                    format_host_port(proxy_host, *proxy_port),
                    format_host_port(target_host, *target_port)
                )
            }
        }
    }

    pub(in crate::proxy) fn tls_host(&self) -> &str {
        match self {
            UpstreamRoute::Direct { host, .. }
            | UpstreamRoute::Socks5 {
                target_host: host, ..
            } => host,
            UpstreamRoute::ProxyChain { hops, .. } => hops[0].host(),
            UpstreamRoute::HttpProxy { proxy_host, .. }
            | UpstreamRoute::HttpsProxy { proxy_host, .. } => proxy_host,
        }
    }

    pub(in crate::proxy) fn uses_absolute_form(&self) -> bool {
        match self {
            UpstreamRoute::HttpProxy { .. } | UpstreamRoute::HttpsProxy { .. } => true,
            UpstreamRoute::ProxyChain { hops, .. } => {
                matches!(
                    hops.last(),
                    Some(ProxyHop::Http { .. } | ProxyHop::Https { .. })
                )
            }
            _ => false,
        }
    }

    pub(in crate::proxy) fn uses_absolute_form_for_url(&self, url: &UrlParts) -> bool {
        self.uses_absolute_form() && !self.uses_proxy_tunnel_for_https_origin(url)
    }

    pub(in crate::proxy) fn uses_proxy_tunnel_for_https_origin(&self, url: &UrlParts) -> bool {
        url.scheme == "https"
            && matches!(
                self,
                UpstreamRoute::HttpProxy { .. }
                    | UpstreamRoute::ProxyChain { .. }
                    | UpstreamRoute::HttpsProxy { .. }
            )
    }

    pub(in crate::proxy) fn uses_tls_to_proxy(&self) -> bool {
        matches!(self, UpstreamRoute::HttpsProxy { .. })
    }

    pub(in crate::proxy) fn tunnel_target_addr(&self) -> String {
        match self {
            UpstreamRoute::Direct { host, port }
            | UpstreamRoute::HttpProxy {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::ProxyChain {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::HttpsProxy {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::Socks5 {
                target_host: host,
                target_port: port,
                ..
            } => format_host_port(host, *port),
        }
    }

    pub(in crate::proxy) fn tunnel_session_label(&self) -> String {
        match self {
            UpstreamRoute::Direct { .. } => self.tunnel_target_addr(),
            UpstreamRoute::HttpProxy { .. }
            | UpstreamRoute::ProxyChain { .. }
            | UpstreamRoute::HttpsProxy { .. } => {
                format!("{}->{}", self.session_label(), self.tunnel_target_addr())
            }
            UpstreamRoute::Socks5 { .. } => self.session_label(),
        }
    }

    pub(in crate::proxy) fn tunnel_target_parts(&self) -> (&str, u16) {
        match self {
            UpstreamRoute::Direct { host, port }
            | UpstreamRoute::HttpProxy {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::ProxyChain {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::HttpsProxy {
                target_host: host,
                target_port: port,
                ..
            }
            | UpstreamRoute::Socks5 {
                target_host: host,
                target_port: port,
                ..
            } => (host, *port),
        }
    }

    pub(in crate::proxy) fn is_direct(&self) -> bool {
        matches!(self, UpstreamRoute::Direct { .. })
    }
}
