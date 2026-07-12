use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlParts {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
}

impl UrlParts {
    pub fn parse(input: &str) -> Result<Self, String> {
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| format!("url must include scheme: {input}"))?;
        let scheme = scheme.to_ascii_lowercase();
        let (authority, path_and_query) = match rest.find(['/', '?']) {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err("url host is empty".to_string());
        }

        let (host, port) = split_host_port(authority);
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if host.is_empty() {
            return Err("url host is empty".to_string());
        }

        let (path, query) = if let Some((path, query)) = path_and_query.split_once('?') {
            let path = if path.is_empty() { "/" } else { path };
            (path.to_string(), Some(query.to_string()))
        } else if let Some(query) = path_and_query.strip_prefix('?') {
            ("/".to_string(), Some(query.to_string()))
        } else {
            (path_and_query.to_string(), None)
        };

        Ok(Self {
            scheme,
            host,
            port,
            path,
            query,
        })
    }

    pub fn effective_port(&self) -> Option<u16> {
        self.port.or(match self.scheme.as_str() {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            "tunnel" => self.port,
            _ => None,
        })
    }

    pub fn origin_form(&self) -> String {
        match &self.query {
            Some(query) if !query.is_empty() => format!("{}?{}", self.path, query),
            _ => self.path.clone(),
        }
    }
}
