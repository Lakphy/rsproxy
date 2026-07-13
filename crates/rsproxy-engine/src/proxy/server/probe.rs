use super::*;

const PROBE_BUFFER_SIZE: usize = 64;
const MAX_METHOD_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConnectProtocol {
    Tls,
    Http,
    Unknown,
    Timeout,
    Closed,
}

pub(super) fn detect(stream: &mut TcpStream, timeout: Duration) -> io::Result<ConnectProtocol> {
    let original_timeout = stream.read_timeout()?;
    let result = detect_inner(stream, timeout);
    let restore = stream.set_read_timeout(original_timeout);
    match (result, restore) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(stage_error("connect_probe_restore", error)),
        (Ok(protocol), Ok(())) => Ok(protocol),
    }
}

fn detect_inner(stream: &mut TcpStream, timeout: Duration) -> io::Result<ConnectProtocol> {
    let started = Instant::now();
    let mut buffer = [0u8; PROBE_BUFFER_SIZE];
    loop {
        let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
            return Ok(ConnectProtocol::Timeout);
        };
        if remaining.is_zero() {
            return Ok(ConnectProtocol::Timeout);
        }
        stream.set_read_timeout(Some(remaining))?;
        let size = match stream.peek(&mut buffer) {
            Ok(0) => return Ok(ConnectProtocol::Closed),
            Ok(size) => size,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(ConnectProtocol::Timeout);
            }
            Err(error) => return Err(stage_error("connect_probe", error)),
        };
        match classify_prefix(&buffer[..size]) {
            PrefixState::Protocol(protocol) => return Ok(protocol),
            PrefixState::NeedMore => thread::sleep(Duration::from_millis(1)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrefixState {
    Protocol(ConnectProtocol),
    NeedMore,
}

fn classify_prefix(input: &[u8]) -> PrefixState {
    if input.first() == Some(&0x16) {
        if input.len() < 3 {
            return PrefixState::NeedMore;
        }
        return PrefixState::Protocol(if input[1] == 0x03 && input[2] <= 0x04 {
            ConnectProtocol::Tls
        } else {
            ConnectProtocol::Unknown
        });
    }

    let Some(space) = input.iter().position(|byte| *byte == b' ') else {
        return if input.len() <= MAX_METHOD_SIZE && input.iter().all(|byte| is_token(*byte)) {
            PrefixState::NeedMore
        } else {
            PrefixState::Protocol(ConnectProtocol::Unknown)
        };
    };
    if space == 0 || space > MAX_METHOD_SIZE || !input[..space].iter().all(|byte| is_token(*byte)) {
        return PrefixState::Protocol(ConnectProtocol::Unknown);
    }
    let target = &input[space + 1..];
    if target.is_empty() {
        return PrefixState::NeedMore;
    }
    if target[0] == b'/' || target[0] == b'*' {
        return PrefixState::Protocol(ConnectProtocol::Http);
    }
    for scheme in [b"http://".as_slice(), b"https://".as_slice()] {
        if scheme.starts_with(target) {
            return PrefixState::NeedMore;
        }
        if target.starts_with(scheme) {
            return PrefixState::Protocol(ConnectProtocol::Http);
        }
    }
    PrefixState::Protocol(ConnectProtocol::Unknown)
}

fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
#[path = "tests/probe.rs"]
mod tests;
