use std::io::{self, Read};

pub(super) fn read_response_body(
    reader: &mut impl Read,
    method: &str,
    status: u16,
    headers: &[(String, String)],
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<(usize, Vec<u8>)> {
    if method.eq_ignore_ascii_case("HEAD")
        || (100..200).contains(&status)
        || matches!(status, 204 | 304)
    {
        return Ok((0, Vec::new()));
    }
    if header_tokens(headers, "transfer-encoding").any(|token| token == "chunked") {
        return read_chunked_body(reader, max_header_size, max_header_count);
    }
    if let Some(length) = content_length(headers)? {
        return read_fixed_body(reader, length);
    }
    read_close_delimited_body(reader)
}

fn content_length(headers: &[(String, String)]) -> io::Result<Option<usize>> {
    let mut lengths = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid replay Content-Length")
            })
        });
    let Some(first) = lengths.next().transpose()? else {
        return Ok(None);
    };
    for length in lengths {
        if length? != first {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conflicting replay Content-Length values",
            ));
        }
    }
    Ok(Some(first))
}

fn header_tokens<'a>(
    headers: &'a [(String, String)],
    name: &'a str,
) -> impl Iterator<Item = String> + 'a {
    headers
        .iter()
        .filter(move |(header, _)| header.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
}

fn read_fixed_body(reader: &mut impl Read, length: usize) -> io::Result<(usize, Vec<u8>)> {
    let mut capture = BodyCapture::default();
    let mut remaining = length;
    let mut buffer = [0u8; 8 * 1024];
    while remaining != 0 {
        let read_limit = remaining.min(buffer.len());
        let size = reader.read(&mut buffer[..read_limit])?;
        if size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "replay response ended before Content-Length bytes arrived",
            ));
        }
        capture.push(&buffer[..size]);
        remaining -= size;
    }
    Ok(capture.finish())
}

fn read_close_delimited_body(reader: &mut impl Read) -> io::Result<(usize, Vec<u8>)> {
    let mut capture = BodyCapture::default();
    let mut buffer = [0u8; 8 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(capture.finish()),
            Ok(size) => capture.push(&buffer[..size]),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                return Ok(capture.finish());
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_chunked_body(
    reader: &mut impl Read,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<(usize, Vec<u8>)> {
    let mut capture = BodyCapture::default();
    loop {
        let line = read_crlf_line(reader, max_header_size)?;
        let size = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid replay chunk size"))?;
        if size == 0 {
            for index in 0..=max_header_count {
                let trailer = read_crlf_line(reader, max_header_size)?;
                if trailer.is_empty() {
                    return Ok(capture.finish());
                }
                if index == max_header_count {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many replay response trailers",
                    ));
                }
            }
        }
        let (read, bytes) = read_fixed_body(reader, size)?;
        capture.total = capture.total.saturating_add(read);
        capture.append_preview(&bytes);
        let mut terminator = [0u8; 2];
        reader.read_exact(&mut terminator)?;
        if terminator != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid replay chunk terminator",
            ));
        }
    }
}

fn read_crlf_line(reader: &mut impl Read, limit: usize) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte)?;
        line.push(byte[0]);
        if line.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "replay chunk metadata exceeds header size limit",
            ));
        }
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return String::from_utf8(line).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 chunk metadata")
            });
        }
    }
}

#[derive(Default)]
struct BodyCapture {
    total: usize,
    preview: Vec<u8>,
}

impl BodyCapture {
    fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        self.append_preview(bytes);
    }

    fn append_preview(&mut self, bytes: &[u8]) {
        const LIMIT: usize = 64 * 1024;
        let available = LIMIT.saturating_sub(self.preview.len());
        self.preview
            .extend_from_slice(&bytes[..bytes.len().min(available)]);
    }

    fn finish(self) -> (usize, Vec<u8>) {
        (self.total, self.preview)
    }
}

#[cfg(test)]
mod tests;
