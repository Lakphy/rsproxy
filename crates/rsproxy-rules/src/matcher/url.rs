use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
/// Normalized components of an absolute URL used by rule matching.
///
/// Parsing lowercases scheme and host, removes IPv6 brackets from the stored
/// host, defaults an absent path to `/`, and preserves whether a query exists.
pub struct UrlParts {
    /// Lowercased scheme without `://`.
    pub scheme: String,
    /// Lowercased hostname or unbracketed IPv6 literal.
    pub host: String,
    /// Explicit authority port; scheme defaults are exposed by [`Self::effective_port`].
    pub port: Option<u16>,
    /// Origin-form pathname, always non-empty.
    pub path: String,
    /// Query text without `?`; an explicitly empty query remains `Some("")`.
    pub query: Option<String>,
}

impl UrlParts {
    /// Parses a strict absolute URL and rejects missing schemes, hosts, or invalid ports.
    pub fn parse(input: &str) -> Result<Self, RuleModelError> {
        let (scheme, rest) = input.split_once("://").ok_or_else(|| {
            RuleModelError::syntax("URL", format!("url must include scheme: {input}"))
        })?;
        let scheme = scheme.to_ascii_lowercase();
        let (authority, path_and_query) = match rest.find(['/', '?']) {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(RuleModelError::empty("URL host", "url host is empty"));
        }

        let (host, port) = split_host_port(authority);
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if host.is_empty() {
            return Err(RuleModelError::empty("URL host", "url host is empty"));
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

    /// Returns the explicit port or the standard HTTP(S)/WebSocket scheme default.
    pub fn effective_port(&self) -> Option<u16> {
        self.port.or(match self.scheme.as_str() {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            "tunnel" => self.port,
            _ => None,
        })
    }

    /// Reconstructs the path plus a non-empty query for an HTTP request target.
    pub fn origin_form(&self) -> String {
        match &self.query {
            Some(query) if !query.is_empty() => format!("{}?{}", self.path, query),
            _ => self.path.clone(),
        }
    }
}
