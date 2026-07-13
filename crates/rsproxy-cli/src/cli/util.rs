use crate::{CliError, CliResult, ConfigError};
use rsproxy_trace::TraceSpillCompression;
use std::io::{self, Read};

pub(super) fn read_stdin() -> CliResult<String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|source| CliError::io("read stdin", source))?;
    Ok(input)
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
