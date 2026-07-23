use crate::{CliError, CliResult, ConfigError};
use rsproxy_trace::TraceSpillCompression;
use std::io::{self, Read};
use std::path::Path;

pub(super) fn read_stdin_bounded(limit: usize, label: &str) -> CliResult<String> {
    read_utf8_bounded(io::stdin().lock(), limit, label, "read stdin")
}

pub(super) fn read_utf8_file_bounded(path: &Path, limit: usize, label: &str) -> CliResult<String> {
    let file = std::fs::File::open(path)
        .map_err(|source| CliError::io(format!("read {label} {}", path.display()), source))?;
    read_utf8_bounded(
        file,
        limit,
        label,
        &format!("read {label} {}", path.display()),
    )
}

fn read_utf8_bounded(
    reader: impl Read,
    limit: usize,
    label: &str,
    io_context: &str,
) -> CliResult<String> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| CliError::io(io_context, source))?;
    if bytes.len() > limit {
        return Err(CliError::Usage(format!(
            "{label} exceeds the {limit}-byte limit"
        )));
    }
    String::from_utf8(bytes).map_err(|source| {
        CliError::io(
            io_context,
            io::Error::new(io::ErrorKind::InvalidData, source),
        )
    })
}

pub(super) fn parse_size(input: &str) -> Result<usize, ConfigError> {
    let lower = input.trim().to_ascii_lowercase();
    if let Some(raw) = lower.strip_suffix("kb") {
        Ok(parse_usize(raw, input)? * 1024)
    } else if let Some(raw) = lower.strip_suffix("mb") {
        Ok(parse_usize(raw, input)? * 1024 * 1024)
    } else if let Some(raw) = lower.strip_suffix("gb") {
        Ok(parse_usize(raw, input)? * 1024 * 1024 * 1024)
    } else if let Some(raw) = lower.strip_suffix('b') {
        parse_usize(raw, input)
    } else {
        parse_usize(&lower, input)
    }
}

fn parse_usize(raw: &str, original: &str) -> Result<usize, ConfigError> {
    raw.parse::<usize>()
        .map_err(|source| ConfigError::InvalidInteger {
            field: "size",
            input: original.to_string(),
            source,
        })
}

pub(super) fn parse_trace_spill_compression(
    input: &str,
) -> Result<TraceSpillCompression, ConfigError> {
    let value = input.trim().to_ascii_lowercase();
    match value.as_str() {
        "none" | "off" | "false" => Ok(TraceSpillCompression::None),
        "zstd" | "zstd:1" => Ok(TraceSpillCompression::Zstd { level: 1 }),
        _ => {
            let Some(level) = value.strip_prefix("zstd:") else {
                return Err(ConfigError::Invalid(
                    "--trace-spill-compression must be none or zstd[:level]".to_string(),
                ));
            };
            let level = level
                .parse::<i32>()
                .map_err(|source| ConfigError::InvalidInteger {
                    field: "--trace-spill-compression zstd level",
                    input: level.to_string(),
                    source,
                })?;
            if !(1..=22).contains(&level) {
                return Err(ConfigError::Invalid(
                    "--trace-spill-compression zstd level must be between 1 and 22".to_string(),
                ));
            }
            Ok(TraceSpillCompression::Zstd { level })
        }
    }
}

pub(super) fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests;
