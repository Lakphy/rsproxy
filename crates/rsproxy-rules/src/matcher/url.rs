use super::*;
use std::net::Ipv6Addr;

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
        if !valid_absolute_scheme(scheme) {
            return Err(RuleModelError::invalid(
                "URL scheme",
                format!("invalid URL scheme `{scheme}`"),
            ));
        }
        if input.contains('#') {
            return Err(RuleModelError::invalid(
                "URL fragment",
                "absolute request URLs must not include a fragment",
            ));
        }
        let scheme = scheme.to_ascii_lowercase();
        let (authority, path_and_query) = match rest.find(['/', '?']) {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(RuleModelError::empty("URL host", "url host is empty"));
        }
        if authority.contains('@') {
            return Err(RuleModelError::invalid(
                "URL authority",
                "URL user information is not supported",
            ));
        }

        let (host, port) = parse_absolute_authority(authority)?;
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if host.is_empty() {
            return Err(RuleModelError::empty("URL host", "url host is empty"));
        }
        if host
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(RuleModelError::invalid(
                "URL host",
                "URL host must not contain whitespace or control characters",
            ));
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

fn valid_absolute_scheme(input: &str) -> bool {
    let mut bytes = input.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn parse_absolute_authority(authority: &str) -> Result<(&str, Option<u16>), RuleModelError> {
    if authority.starts_with('[') {
        let end = authority.find(']').ok_or_else(|| {
            RuleModelError::syntax("URL IPv6 authority", "IPv6 URL host is missing closing `]`")
        })?;
        let host = &authority[1..end];
        host.parse::<Ipv6Addr>().map_err(|_| {
            RuleModelError::invalid(
                "URL IPv6 authority",
                format!("invalid IPv6 URL host `{host}`"),
            )
        })?;
        let tail = &authority[end + 1..];
        let port = if tail.is_empty() {
            None
        } else {
            let raw = tail.strip_prefix(':').ok_or_else(|| {
                RuleModelError::invalid(
                    "URL authority",
                    format!("invalid URL authority `{authority}`"),
                )
            })?;
            Some(parse_absolute_port(raw)?)
        };
        return Ok((&authority[..=end], port));
    }
    if authority.contains(['[', ']']) {
        return Err(RuleModelError::invalid(
            "URL authority",
            format!("invalid URL authority `{authority}`"),
        ));
    }
    match authority.rsplit_once(':') {
        Some((host, raw_port)) if !host.contains(':') => {
            if host.is_empty() {
                return Err(RuleModelError::empty("URL host", "url host is empty"));
            }
            Ok((host, Some(parse_absolute_port(raw_port)?)))
        }
        Some(_) => Err(RuleModelError::syntax(
            "URL IPv6 authority",
            "IPv6 URL hosts must use brackets",
        )),
        None => Ok((authority, None)),
    }
}

fn parse_absolute_port(input: &str) -> Result<u16, RuleModelError> {
    if input.is_empty() {
        return Err(RuleModelError::empty("URL port", "URL port is empty"));
    }
    let port = input.parse::<u16>().map_err(|source| {
        RuleModelError::integer(
            "URL port",
            input,
            format!("invalid URL port `{input}`"),
            source,
        )
    })?;
    if port == 0 {
        return Err(RuleModelError::constraint(
            "URL port",
            "URL port must be 1..65535",
        ));
    }
    Ok(port)
}

/// Validates one HTTP `Location` URI-reference without normalizing its text.
pub fn validate_redirect_location(value: &str) -> Result<(), RuleModelError> {
    if value.is_empty() {
        return Err(RuleModelError::empty(
            "redirect location",
            "redirect location must not be empty",
        ));
    }
    if value
        .bytes()
        .any(|byte| byte <= b' ' || byte == 0x7f || byte == b'\\')
    {
        return Err(RuleModelError::invalid(
            "redirect location",
            "redirect location must not contain whitespace, controls, or backslashes",
        ));
    }
    validate_percent_encoding(value)?;
    let reference = value.split_once('#').map_or(value, |(base, _)| base);
    if reference.is_empty() {
        return Ok(());
    }
    if let Some((scheme, _)) = reference.split_once(':')
        && valid_absolute_scheme(scheme)
    {
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return Err(RuleModelError::unsupported(
                "redirect location scheme",
                "absolute redirect locations must use http or https",
            ));
        }
        UrlParts::parse(reference)?;
    } else if let Some(authority) = reference.strip_prefix("//") {
        UrlParts::parse(&format!("http://{authority}"))?;
    } else if reference
        .split(['/', '?'])
        .next()
        .is_some_and(|segment| segment.contains(':'))
    {
        return Err(RuleModelError::invalid(
            "redirect location",
            "a relative redirect's first path segment must not contain `:`",
        ));
    }
    Ok(())
}

fn validate_percent_encoding(value: &str) -> Result<(), RuleModelError> {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return Err(RuleModelError::invalid(
                "redirect location",
                "redirect location contains an invalid percent escape",
            ));
        }
        index += 3;
    }
    Ok(())
}
