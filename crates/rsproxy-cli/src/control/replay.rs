use crate::{http, json};
use rsproxy_rules::UrlParts;
use std::io::{Read, Write};
use std::net::TcpStream;

pub(super) fn replay_session(
    session: &rsproxy_trace::Session,
    max_header_size: usize,
    max_header_count: usize,
) -> std::io::Result<String> {
    let url = UrlParts::parse(&session.url)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    if url.scheme != "http" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "replay currently supports http URLs only",
        ));
    }
    let port = url.effective_port().unwrap_or(80);
    let addr = format!("{}:{port}", url.host);
    let mut upstream = TcpStream::connect(&addr)?;
    let mut headers = session.req_headers.clone();
    http::remove_header(&mut headers, "proxy-connection");
    http::remove_header(&mut headers, "connection");
    http::remove_header(&mut headers, "content-length");
    http::set_header(&mut headers, "Host", host_header(&url));
    http::set_header(&mut headers, "Connection", "close".to_string());
    if !session.req_body_head.is_empty() {
        http::set_header(
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
    )?;
    for (name, value) in &headers {
        write!(upstream, "{name}: {value}\r\n")?;
    }
    write!(upstream, "\r\n")?;
    if !session.req_body_head.is_empty() {
        upstream.write_all(&session.req_body_head)?;
    }

    let head = http::read_response_head(&mut upstream, max_header_size, max_header_count)?;
    let mut body = Vec::new();
    upstream.read_to_end(&mut body)?;
    Ok(format!(
        "{{\"id\":{},\"url\":{},\"status\":{},\"response_bytes\":{},\"headers\":{},\"body_head\":{}}}",
        session.id,
        json::string(&session.url),
        head.status,
        body.len(),
        json::headers(&head.headers),
        json::string(&String::from_utf8_lossy(
            &body.iter().copied().take(64 * 1024).collect::<Vec<_>>()
        ))
    ))
}

fn host_header(url: &UrlParts) -> String {
    match (url.port, url.scheme.as_str()) {
        (Some(80), "http" | "ws") | (Some(443), "https" | "wss") | (None, _) => url.host.clone(),
        (Some(port), _) => format!("{}:{port}", url.host),
    }
}
