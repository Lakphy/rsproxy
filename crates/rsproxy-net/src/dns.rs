use crate::runtime::h2_runtime;
use crate::{NetError, NetResult, NetStage, ProtocolErrorKind};
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{
    ConnectionConfig, LookupIpStrategy, NameServerConfig, ResolverConfig,
};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const DNS_CACHE_CAPACITY: u64 = 8_192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsConfig {
    pub servers: Vec<SocketAddr>,
    pub timeout: Duration,
    pub cache_ttl: Duration,
}

pub struct DnsResolver {
    resolver: TokioResolver,
    timeout: Duration,
    stats: DnsStats,
}

#[derive(Default)]
struct DnsStats {
    lookups: AtomicU64,
    successes: AtomicU64,
    failures: AtomicU64,
    timeouts: AtomicU64,
    literal_bypasses: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DnsStatsSnapshot {
    pub lookups: u64,
    pub successes: u64,
    pub failures: u64,
    pub timeouts: u64,
    pub literal_bypasses: u64,
}

impl DnsResolver {
    pub fn new(config: &DnsConfig) -> NetResult<Self> {
        let mut builder = if config.servers.is_empty() {
            TokioResolver::builder_tokio().map_err(|error| NetError::Protocol {
                kind: ProtocolErrorKind::UnexpectedMessage,
                stage: NetStage::Dns,
                message: format!("load system resolver configuration: {error}"),
            })?
        } else {
            let name_servers = config
                .servers
                .iter()
                .copied()
                .map(name_server_config)
                .collect();
            TokioResolver::builder_with_config(
                ResolverConfig::from_parts(None, Vec::new(), name_servers),
                TokioRuntimeProvider::default(),
            )
        };
        let options = builder.options_mut();
        options.cache_size = if config.cache_ttl.is_zero() {
            0
        } else {
            DNS_CACHE_CAPACITY
        };
        options.positive_max_ttl = Some(config.cache_ttl);
        options.negative_max_ttl = Some(config.cache_ttl);
        options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
        options.try_tcp_on_error = true;
        options.timeout = config
            .timeout
            .checked_add(Duration::from_millis(250))
            .unwrap_or(config.timeout);

        let resolver = builder.build().map_err(|error| NetError::Protocol {
            kind: ProtocolErrorKind::UnexpectedMessage,
            stage: NetStage::Dns,
            message: format!("build resolver: {error}"),
        })?;
        Ok(Self {
            resolver,
            timeout: config.timeout,
            stats: DnsStats::default(),
        })
    }

    pub fn resolve_socket_addrs(&self, target: &str) -> io::Result<Vec<SocketAddr>> {
        self.resolve_socket_addrs_with_timeout(target, self.timeout)
    }

    pub fn resolve_socket_addrs_with_timeout(
        &self,
        target: &str,
        timeout: Duration,
    ) -> io::Result<Vec<SocketAddr>> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stage=dns: timeout must be greater than zero",
            ));
        }
        let (host, port) = split_socket_target(target)?;
        if let Ok(ip) = IpAddr::from_str(&host) {
            self.stats.literal_bypasses.fetch_add(1, Ordering::Relaxed);
            return Ok(vec![SocketAddr::new(ip, port)]);
        }

        self.stats.lookups.fetch_add(1, Ordering::Relaxed);
        let lookup = h2_runtime()?.block_on(async {
            tokio::time::timeout(timeout, self.resolver.lookup_ip(host.as_str())).await
        });
        match lookup {
            Err(_) => {
                self.stats.failures.fetch_add(1, Ordering::Relaxed);
                self.stats.timeouts.fetch_add(1, Ordering::Relaxed);
                Err(dns_timeout_error(timeout, &host))
            }
            Ok(Err(err)) => {
                self.stats.failures.fetch_add(1, Ordering::Relaxed);
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("stage=dns: failed to resolve {host}: {err}"),
                ))
            }
            Ok(Ok(lookup)) => {
                let mut addresses = Vec::new();
                for ip in lookup.iter() {
                    let address = SocketAddr::new(ip, port);
                    if !addresses.contains(&address) {
                        addresses.push(address);
                    }
                }
                if addresses.is_empty() {
                    self.stats.failures.fetch_add(1, Ordering::Relaxed);
                    return Err(io::Error::new(
                        io::ErrorKind::AddrNotAvailable,
                        format!("stage=dns: no addresses resolved for {host}"),
                    ));
                }
                self.stats.successes.fetch_add(1, Ordering::Relaxed);
                Ok(addresses)
            }
        }
    }

    pub fn stats(&self) -> DnsStatsSnapshot {
        DnsStatsSnapshot {
            lookups: self.stats.lookups.load(Ordering::Relaxed),
            successes: self.stats.successes.load(Ordering::Relaxed),
            failures: self.stats.failures.load(Ordering::Relaxed),
            timeouts: self.stats.timeouts.load(Ordering::Relaxed),
            literal_bypasses: self.stats.literal_bypasses.load(Ordering::Relaxed),
        }
    }
}

fn name_server_config(address: SocketAddr) -> NameServerConfig {
    let mut udp = ConnectionConfig::udp();
    udp.port = address.port();
    let mut tcp = ConnectionConfig::tcp();
    tcp.port = address.port();
    NameServerConfig::new(address.ip(), true, vec![udp, tcp])
}

fn split_socket_target(target: &str) -> io::Result<(String, u16)> {
    if let Ok(address) = SocketAddr::from_str(target) {
        return Ok((address.ip().to_string(), address.port()));
    }
    let (host, port) = target.rsplit_once(':').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stage=dns: target has no port: {target}"),
        )
    })?;
    let port = port.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stage=dns: invalid target port: {target}"),
        )
    })?;
    let host = host.trim_matches(['[', ']']);
    if host.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stage=dns: target has no host: {target}"),
        ));
    }
    Ok((host.to_string(), port))
}

fn dns_timeout_error(timeout: Duration, host: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "stage=dns: timeout after {}ms resolving {host}",
            timeout.as_millis()
        ),
    )
}
