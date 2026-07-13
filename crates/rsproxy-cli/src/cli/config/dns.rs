use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

pub(super) fn parse_dns_servers(values: &[String]) -> Result<Vec<SocketAddr>, ConfigError> {
    let mut servers = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(ConfigError::Invalid(
                    "--dns-server contains an empty server".to_string(),
                ));
            }
            let address = parse_dns_server(raw)?;
            if !servers.contains(&address) {
                servers.push(address);
            }
        }
    }
    Ok(servers)
}

fn parse_dns_server(value: &str) -> Result<SocketAddr, ConfigError> {
    if let Ok(address) = SocketAddr::from_str(value) {
        return Ok(address);
    }
    let ip_text = value.trim_matches(['[', ']']);
    if let Ok(ip) = IpAddr::from_str(ip_text) {
        return Ok(SocketAddr::new(ip, 53));
    }
    Err(ConfigError::Invalid(format!(
        "invalid --dns-server `{value}`; expected an IP address with optional port"
    )))
}

#[cfg(test)]
#[path = "dns/tests.rs"]
mod tests;
use crate::ConfigError;
