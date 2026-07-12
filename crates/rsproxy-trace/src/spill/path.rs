use super::TraceSpillCompression;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn segment_path(dir: &Path, index: u64, compression: TraceSpillCompression) -> PathBuf {
    match compression {
        TraceSpillCompression::None => dir.join(format!("seg-{index:012}.ndjson")),
        TraceSpillCompression::Zstd { .. } => dir.join(format!("seg-{index:012}.ndjson.zst")),
    }
}

pub(super) fn segment_index_path(
    dir: &Path,
    index: u64,
    compression: TraceSpillCompression,
) -> PathBuf {
    index_path_for_segment(&segment_path(dir, index, compression))
}

pub(crate) fn index_path_for_segment(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".idx");
    path.with_file_name(name)
}

pub(super) fn parse_segment_index(name: &str) -> Option<u64> {
    parse_segment_name(name).map(|segment| segment.index)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ParsedSegmentName {
    pub(super) index: u64,
    pub(super) compression: TraceSpillCompression,
}

pub(super) fn parse_segment_name(name: &str) -> Option<ParsedSegmentName> {
    if let Some(stem) = name.strip_suffix(".ndjson.zst") {
        return Some(ParsedSegmentName {
            index: stem.strip_prefix("seg-")?.parse().ok()?,
            compression: TraceSpillCompression::Zstd { level: 1 },
        });
    }
    Some(ParsedSegmentName {
        index: name
            .strip_prefix("seg-")?
            .strip_suffix(".ndjson")?
            .parse()
            .ok()?,
        compression: TraceSpillCompression::None,
    })
}

pub(super) fn index_metadata(path: &Path) -> std::io::Result<(u64, u64)> {
    let bytes = match fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(err) => return Err(err),
    };
    let body = fs::read(path)?;
    let entries = body.iter().filter(|byte| **byte == b'\n').count() as u64;
    Ok((bytes, entries))
}
