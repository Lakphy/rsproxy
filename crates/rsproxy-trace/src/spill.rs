use crate::model::Session;
use crate::serialize::spill_session_line;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

mod codec;
mod path;
mod read;

use codec::{crc32, encode_spill_record};
use path::{
    index_metadata, index_path_for_segment, parse_segment_index, parse_segment_name,
    segment_index_path, segment_path,
};
pub(crate) use read::{SpillReadSnapshot, read_verified_snapshot, spill_read_snapshot};

#[cfg(test)]
pub(super) use codec::parse_index_entry;
#[cfg(test)]
pub(super) use path::index_path_for_segment as test_index_path_for_segment;

#[derive(Clone, Debug)]
/// Disk-spill policy applied when completed sessions leave resident memory.
pub struct TraceSpillConfig {
    /// Directory that owns numbered data segments and their integrity indexes.
    pub dir: PathBuf,
    /// Preferred maximum encoded size of each data segment, in bytes.
    ///
    /// A single record larger than this value is retained intact in its own segment.
    pub segment_bytes: u64,
    /// Target combined size for data and index files; zero disables eviction by disk size.
    ///
    /// Whole oldest segments are removed, but the active segment is retained even if it alone
    /// exceeds this budget.
    pub disk_budget_bytes: u64,
    /// Encoding applied independently to each spill record.
    pub compression: TraceSpillCompression,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Per-record compression used by trace spill segments.
pub enum TraceSpillCompression {
    /// Store newline-delimited JSON records without compression.
    None,
    /// Compress each record as an independent zstd frame.
    Zstd {
        /// zstd compression level passed to the encoder.
        level: i32,
    },
}

impl TraceSpillCompression {
    /// Returns the stable lowercase encoding token used in paths and statistics.
    pub fn name(self) -> &'static str {
        match self {
            TraceSpillCompression::None => "none",
            TraceSpillCompression::Zstd { .. } => "zstd",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TraceSpillState {
    pub(super) dir: PathBuf,
    pub(super) segment_bytes: u64,
    pub(super) disk_budget_bytes: u64,
    pub(super) compression: TraceSpillCompression,
    pub(super) next_segment: u64,
    pub(super) segments: VecDeque<SpillSegment>,
    pub(super) bytes_on_disk: u64,
    pub(super) evicted_segments: u64,
    pub(super) initialized: bool,
}

#[derive(Clone, Debug)]
pub(super) struct SpillSegment {
    pub(super) index: u64,
    pub(super) path: PathBuf,
    pub(super) compression: TraceSpillCompression,
    pub(super) bytes: u64,
    pub(super) idx_path: PathBuf,
    pub(super) index_bytes: u64,
    pub(super) indexed_records: u64,
}

impl TraceSpillConfig {
    /// Creates an uncompressed spill policy and clamps `segment_bytes` to at least one byte.
    pub fn new(dir: PathBuf, segment_bytes: u64, disk_budget_bytes: u64) -> Self {
        Self {
            dir,
            segment_bytes: segment_bytes.max(1),
            disk_budget_bytes,
            compression: TraceSpillCompression::None,
        }
    }

    /// Replaces the record compression policy while preserving all size budgets.
    pub fn with_compression(mut self, compression: TraceSpillCompression) -> Self {
        self.compression = compression;
        self
    }
}

impl TraceSpillState {
    pub(super) fn new(config: TraceSpillConfig) -> Self {
        Self {
            dir: config.dir,
            segment_bytes: config.segment_bytes.max(1),
            disk_budget_bytes: config.disk_budget_bytes,
            compression: config.compression,
            next_segment: 1,
            segments: VecDeque::new(),
            bytes_on_disk: 0,
            evicted_segments: 0,
            initialized: false,
        }
    }

    pub(super) fn active_or_next_path(&self) -> PathBuf {
        self.segments
            .back()
            .map(|segment| segment.path.clone())
            .unwrap_or_else(|| segment_path(&self.dir, self.next_segment, self.compression))
    }
}

pub(super) fn append_spill(spill: &mut TraceSpillState, session: &Session) -> std::io::Result<()> {
    ensure_spill_initialized(spill)?;
    let line = spill_session_line(session);
    let record = encode_spill_record(&line, spill.compression)?;
    let record_bytes = record.len() as u64;
    if spill.segments.is_empty()
        || (spill
            .segments
            .back()
            .map(|segment| {
                segment.compression != spill.compression
                    || (segment.bytes > 0 && segment.bytes + record_bytes > spill.segment_bytes)
            })
            .unwrap_or(false))
    {
        start_spill_segment(spill);
    }

    let segment = spill
        .segments
        .back()
        .expect("initialized spill state must contain an active segment");
    let path = segment.path.clone();
    let idx_path = segment.idx_path.clone();
    let offset = segment.bytes;
    let len = line.len() as u64;
    let crc = crc32(line.as_bytes());
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&record)?;
    let index_len = match spill.compression {
        TraceSpillCompression::None => len,
        TraceSpillCompression::Zstd { .. } => record_bytes,
    };
    let index_line = format!("{offset} {index_len} {} {crc:08x}\n", session.id);
    let mut index_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&idx_path)?;
    index_file.write_all(index_line.as_bytes())?;
    if let Some(segment) = spill.segments.back_mut() {
        segment.bytes += record_bytes;
        segment.index_bytes += index_line.len() as u64;
        segment.indexed_records += 1;
    }
    spill.bytes_on_disk += record_bytes + index_line.len() as u64;
    enforce_spill_budget(spill)?;
    Ok(())
}

pub(super) fn ensure_spill_initialized(spill: &mut TraceSpillState) -> std::io::Result<()> {
    if spill.initialized {
        return Ok(());
    }
    fs::create_dir_all(&spill.dir)?;
    let mut segments = Vec::new();
    for entry in fs::read_dir(&spill.dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(index) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| parse_segment_name(name).map(|segment| segment.index))
        else {
            continue;
        };
        let compression = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_segment_name)
            .map(|segment| segment.compression)
            .unwrap_or(TraceSpillCompression::None);
        let bytes = entry.metadata()?.len();
        let idx_path = index_path_for_segment(&path);
        let (index_bytes, indexed_records) = index_metadata(&idx_path)?;
        segments.push(SpillSegment {
            index,
            path,
            compression,
            bytes,
            idx_path,
            index_bytes,
            indexed_records,
        });
    }
    segments.sort_by_key(|segment| segment.index);
    spill.bytes_on_disk = segments
        .iter()
        .map(|segment| segment.bytes + segment.index_bytes)
        .sum();
    spill.next_segment = segments
        .last()
        .map(|segment| segment.index.saturating_add(1))
        .unwrap_or(1);
    spill.segments = segments.into();
    spill.initialized = true;
    enforce_spill_budget(spill)?;
    Ok(())
}

pub(super) fn start_spill_segment(spill: &mut TraceSpillState) {
    let index = spill.next_segment;
    spill.next_segment += 1;
    spill.segments.push_back(SpillSegment {
        index,
        path: segment_path(&spill.dir, index, spill.compression),
        compression: spill.compression,
        bytes: 0,
        idx_path: segment_index_path(&spill.dir, index, spill.compression),
        index_bytes: 0,
        indexed_records: 0,
    });
}

pub(super) fn enforce_spill_budget(spill: &mut TraceSpillState) -> std::io::Result<()> {
    if spill.disk_budget_bytes == 0 {
        return Ok(());
    }
    while spill.bytes_on_disk > spill.disk_budget_bytes && spill.segments.len() > 1 {
        let Some(segment) = spill.segments.pop_front() else {
            break;
        };
        match fs::remove_file(&segment.path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(std::io::Error::new(
                    err.kind(),
                    format!("remove {}: {err}", segment.path.display()),
                ));
            }
        }
        match fs::remove_file(&segment.idx_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(std::io::Error::new(
                    err.kind(),
                    format!("remove {}: {err}", segment.idx_path.display()),
                ));
            }
        }
        spill.bytes_on_disk = spill
            .bytes_on_disk
            .saturating_sub(segment.bytes + segment.index_bytes);
        spill.evicted_segments += 1;
    }
    Ok(())
}

pub(super) fn clear_spill(spill: &mut TraceSpillState) -> std::io::Result<()> {
    fs::create_dir_all(&spill.dir)?;
    for entry in fs::read_dir(&spill.dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_spill = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                parse_segment_index(name).is_some()
                    || parse_segment_name(name.strip_suffix(".idx").unwrap_or(name)).is_some()
                    || name == "sessions.ndjson"
            })
            .unwrap_or(false);
        if !is_spill {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(std::io::Error::new(
                    err.kind(),
                    format!("clear {}: {err}", path.display()),
                ));
            }
        }
    }
    spill.next_segment = 1;
    spill.segments.clear();
    spill.bytes_on_disk = 0;
    spill.evicted_segments = 0;
    spill.initialized = true;
    Ok(())
}
