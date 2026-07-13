use super::*;

pub(super) fn http_proxy_connect_tunnel<S: Read + Write + ?Sized>(
    stream: &mut S,
    target: &str,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<()> {
    write!(
        stream,
        "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n"
    )?;
    let head = http::read_response_head(stream, max_header_size, max_header_count)?;
    if (200..300).contains(&head.status) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "CONNECT {target} returned {} {}",
            head.status, head.reason
        )))
    }
}

pub(super) fn socks5_connect<S: Read + Write + ?Sized>(
    stream: &mut S,
    target_host: &str,
    target_port: u16,
    auth: Option<&SocksAuth>,
) -> io::Result<()> {
    if auth.is_some() {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02])?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00])?;
    }
    let mut method = [0u8; 2];
    stream.read_exact(&mut method)?;
    if method[0] != 0x05 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS5 greeting response",
        ));
    }
    match (method[1], auth) {
        (0x00, _) => {}
        (0x02, Some(auth)) => socks5_username_password_auth(stream, auth)?,
        (0x02, None) => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "SOCKS5 username/password requested but no credentials were configured",
            ));
        }
        (method, _) => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("SOCKS5 method 0x{method:02x} is not supported"),
            ));
        }
    }

    let mut req = vec![0x05, 0x01, 0x00];
    match target_host.parse::<IpAddr>() {
        Ok(IpAddr::V4(addr)) => {
            req.push(0x01);
            req.extend_from_slice(&addr.octets());
        }
        Ok(IpAddr::V6(addr)) => {
            req.push(0x04);
            req.extend_from_slice(&addr.octets());
        }
        Err(_) => {
            let host = target_host.as_bytes();
            if host.len() > u8::MAX as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "SOCKS5 target host is too long",
                ));
            }
            req.push(0x03);
            req.push(host.len() as u8);
            req.extend_from_slice(host);
        }
    }
    req.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&req)?;

    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    if head[0] != 0x05 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS5 connect response",
        ));
    }
    let atyp = head[3];
    match atyp {
        0x01 => read_exact_discard(stream, 4)?,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            read_exact_discard(stream, len[0] as usize)?;
        }
        0x04 => read_exact_discard(stream, 16)?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid SOCKS5 address type 0x{atyp:02x}"),
            ));
        }
    }
    read_exact_discard(stream, 2)?;
    if head[1] != 0x00 {
        return Err(io::Error::other(format!(
            "SOCKS5 connect failed with reply 0x{:02x}",
            head[1]
        )));
    }
    Ok(())
}

pub(super) fn socks5_username_password_auth<S: Read + Write + ?Sized>(
    stream: &mut S,
    auth: &SocksAuth,
) -> io::Result<()> {
    let username = auth.username.as_bytes();
    let password = auth.password.as_bytes();
    if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SOCKS5 username/password must be at most 255 bytes",
        ));
    }
    let mut req = Vec::with_capacity(3 + username.len() + password.len());
    req.push(0x01);
    req.push(username.len() as u8);
    req.extend_from_slice(username);
    req.push(password.len() as u8);
    req.extend_from_slice(password);
    stream.write_all(&req)?;

    let mut res = [0u8; 2];
    stream.read_exact(&mut res)?;
    if res[0] != 0x01 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS5 username/password auth response",
        ));
    }
    if res[1] != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SOCKS5 username/password authentication failed",
        ));
    }
    Ok(())
}

pub(super) fn read_exact_discard<R: Read + ?Sized>(stream: &mut R, len: usize) -> io::Result<()> {
    let mut remaining = len;
    let mut buf = [0u8; 32];
    while remaining > 0 {
        let take = remaining.min(buf.len());
        stream.read_exact(&mut buf[..take])?;
        remaining -= take;
    }
    Ok(())
}

pub(super) fn stage_error(stage: &str, err: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("stage={stage}: {err}"))
}

pub(super) fn stage_io_error(stage: &str, err: io::Error) -> io::Error {
    if is_request_total_timeout(&err) {
        err
    } else {
        io::Error::new(err.kind(), format!("stage={stage}: {err}"))
    }
}
