use super::*;

pub(super) fn throttle_bps(actions: &[ResolvedAction], phase: Phase) -> Option<u64> {
    actions.iter().find_map(|item| match item.action {
        Action::Throttle {
            phase: action_phase,
            bytes_per_sec,
        } if action_phase == phase => Some(bytes_per_sec),
        _ => None,
    })
}

pub(super) fn write_maybe_throttled<W: Write + ?Sized>(
    stream: &mut W,
    body: &[u8],
    bytes_per_sec: Option<u64>,
) -> io::Result<()> {
    ThrottlePacer::new(bytes_per_sec).write(stream, body)
}

pub(super) fn write_maybe_throttled_until<W: Write + ?Sized>(
    stream: &mut W,
    body: &[u8],
    bytes_per_sec: Option<u64>,
    deadline: RequestDeadline,
) -> io::Result<()> {
    ThrottlePacer::new(bytes_per_sec).write_until(stream, body, deadline)
}

pub(super) struct ThrottlePacer {
    bytes_per_sec: Option<u64>,
    next_chunk_at: Option<Instant>,
}

impl ThrottlePacer {
    pub(super) fn new(bytes_per_sec: Option<u64>) -> Self {
        Self {
            bytes_per_sec: bytes_per_sec.map(|value| value.max(1)),
            next_chunk_at: None,
        }
    }

    pub(super) fn write<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        body: &[u8],
    ) -> io::Result<()> {
        self.write_inner(stream, body, None)
    }

    pub(super) fn write_until<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        body: &[u8],
        deadline: RequestDeadline,
    ) -> io::Result<()> {
        self.write_inner(stream, body, Some(deadline))
    }

    fn write_inner<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        body: &[u8],
        deadline: Option<RequestDeadline>,
    ) -> io::Result<()> {
        let Some(bytes_per_sec) = self.bytes_per_sec else {
            return stream.write_all(body);
        };
        let chunk_size = bytes_per_sec.clamp(1, 16 * 1024) as usize;
        for chunk in body.chunks(chunk_size) {
            self.wait_for_next_chunk(deadline)?;
            stream.write_all(chunk)?;
            self.next_chunk_at = Some(
                Instant::now() + Duration::from_secs_f64(chunk.len() as f64 / bytes_per_sec as f64),
            );
        }
        Ok(())
    }

    fn wait_for_next_chunk(&self, deadline: Option<RequestDeadline>) -> io::Result<()> {
        let Some(next_chunk_at) = self.next_chunk_at else {
            return Ok(());
        };
        let Some(wait) = next_chunk_at.checked_duration_since(Instant::now()) else {
            return Ok(());
        };
        if let Some(deadline) = deadline {
            deadline.sleep(wait)
        } else {
            thread::sleep(wait);
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "request_util/tests.rs"]
mod tests;

pub(super) fn absolute_url_for(
    req: &RawRequest,
    https_authority: Option<&str>,
) -> io::Result<String> {
    let absolute = if req.target.contains("://") {
        req.target.clone()
    } else if let Some(authority) = https_authority {
        format!("https://{authority}{}", req.target)
    } else {
        let host = http::header(&req.headers, "host").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "origin-form request missing Host",
            )
        })?;
        format!("http://{host}{}", req.target)
    };
    normalize_websocket_transport_scheme(&absolute)
}

/// Presents Upgrade requests to the rules layer as WebSocket URLs while the
/// forwarding layer continues to use their HTTP(S) transport URL.
pub(super) fn rule_url_for(transport_url: &str, headers: &[(String, String)]) -> String {
    if !is_websocket_request(headers) {
        return transport_url.to_string();
    }
    replace_url_scheme(transport_url, |scheme| match scheme {
        "http" => Some("ws"),
        "https" => Some("wss"),
        _ => None,
    })
    .unwrap_or_else(|| transport_url.to_string())
}

fn normalize_websocket_transport_scheme(url: &str) -> io::Result<String> {
    let parsed =
        UrlParts::parse(url).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(replace_url_scheme(url, |_| match parsed.scheme.as_str() {
        "ws" => Some("http"),
        "wss" => Some("https"),
        _ => None,
    })
    .unwrap_or_else(|| url.to_string()))
}

fn replace_url_scheme(url: &str, replacement: impl FnOnce(&str) -> Option<&str>) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    replacement(&scheme).map(|replacement| format!("{replacement}://{rest}"))
}

pub(super) fn host_header(url: &UrlParts) -> String {
    let host = format_authority_host(&url.host);
    match (url.port, url.scheme.as_str()) {
        (Some(80), "http" | "ws") | (Some(443), "https" | "wss") | (None, _) => host,
        (Some(port), _) => format!("{host}:{port}"),
    }
}

pub(super) fn format_host_port(host: &str, port: u16) -> String {
    format!("{}:{port}", format_authority_host(host))
}

pub(super) fn format_authority_host(host: &str) -> String {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

pub(super) fn split_addr(input: &str, default_port: u16) -> (String, u16) {
    if let Some((host, port)) = input.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
    {
        return (host.trim_matches(['[', ']']).to_string(), port);
    }
    (input.trim_matches(['[', ']']).to_string(), default_port)
}

pub(super) fn split_socks_auth(input: &str) -> (Option<SocksAuth>, String) {
    let Some((auth, addr)) = input.rsplit_once('@') else {
        return (None, input.to_string());
    };
    let Some((username, password)) = auth.split_once(':') else {
        return (None, input.to_string());
    };
    (
        Some(SocksAuth {
            username: username.to_string(),
            password: password.to_string(),
        }),
        addr.to_string(),
    )
}
