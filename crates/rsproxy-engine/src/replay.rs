use crate::{EngineError, EngineResult, ReplayResponse};
use rsproxy_rules::UrlParts;
use std::io::{Read, Write};
use std::net::TcpStream;

pub(crate) fn replay_session(
    session: &rsproxy_trace::Session,
    max_header_size: usize,
    max_header_count: usize,
) -> EngineResult<ReplayResponse> {
    let url = UrlParts::parse(&session.url)?;
    if url.scheme != "http" {
        return Err(EngineError::Unsupported(
            "replay currently supports http URLs only".to_string(),
        ));
    }
    let port = url.effective_port().unwrap_or(80);
    let address = format!("{}:{port}", url.host);
    let mut upstream = TcpStream::connect(&address).map_err(|source| EngineError::Io {
        context: format!("connect replay origin {address}"),
        source,
    })?;
    let mut headers = session.req_headers.clone();
    rsproxy_net::remove_header(&mut headers, "proxy-connection");
    rsproxy_net::remove_header(&mut headers, "connection");
    rsproxy_net::remove_header(&mut headers, "content-length");
    rsproxy_net::set_header(&mut headers, "Host", host_header(&url));
    rsproxy_net::set_header(&mut headers, "Connection", "close".to_string());
    if !session.req_body_head.is_empty() {
        rsproxy_net::set_header(
            &mut headers,
            "Content-Length",
            session.req_body_head.len().to_string(),
        );
    }

    write!(
        upstream,
        "{} {} HTTP/1.1\r\n",
        session.method,
        url.origin_form()
    )
    .map_err(|source| replay_io("write replay request line", source))?;
    for (name, value) in &headers {
        write!(upstream, "{name}: {value}\r\n")
            .map_err(|source| replay_io("write replay request header", source))?;
    }
    write!(upstream, "\r\n").map_err(|source| replay_io("finish replay request head", source))?;
    if !session.req_body_head.is_empty() {
        upstream
            .write_all(&session.req_body_head)
            .map_err(|source| replay_io("write replay request body", source))?;
    }

    let head = rsproxy_net::read_response_head(&mut upstream, max_header_size, max_header_count)
        .map_err(|source| replay_io("read replay response head", source))?;
    let mut body = Vec::new();
    upstream
        .read_to_end(&mut body)
        .map_err(|source| replay_io("read replay response body", source))?;
    Ok(ReplayResponse {
        status: head.status,
        response_bytes: body.len(),
        headers: head.headers,
        body_head: body.into_iter().take(64 * 1024).collect(),
    })
}

fn replay_io(context: &str, source: std::io::Error) -> EngineError {
    EngineError::Io {
        context: context.to_string(),
        source,
    }
}

fn host_header(url: &UrlParts) -> String {
    match (url.port, url.scheme.as_str()) {
        (Some(80), "http" | "ws") | (Some(443), "https" | "wss") | (None, _) => url.host.clone(),
        (Some(port), _) => format!("{}:{port}", url.host),
    }
}
