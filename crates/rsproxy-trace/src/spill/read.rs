use super::codec::{crc32, decode_indexed_record, parse_index_entry};
use super::{SpillSegment, TraceSpillCompression, TraceSpillState, ensure_spill_initialized};
use std::fs::File;
use std::io::{self, Read};

pub(crate) struct SpillReadSnapshot {
    segments: Vec<SpillReadSegment>,
}

struct SpillReadSegment {
    data: File,
    index: Option<File>,
    compression: TraceSpillCompression,
    data_len: u64,
    index_len: u64,
}

pub(crate) fn spill_read_snapshot(
    spill: Option<&mut TraceSpillState>,
) -> io::Result<SpillReadSnapshot> {
    let Some(spill) = spill else {
        return Ok(SpillReadSnapshot {
            segments: Vec::new(),
        });
    };
    ensure_spill_initialized(spill)?;
    let segments = spill
        .segments
        .iter()
        .map(SpillReadSegment::open)
        .collect::<io::Result<Vec<_>>>()?;
    Ok(SpillReadSnapshot { segments })
}

pub(crate) fn read_verified_snapshot(snapshot: SpillReadSnapshot) -> io::Result<(Vec<u8>, u64)> {
    let mut body = Vec::new();
    let mut corrupt = 0u64;
    for segment in snapshot.segments {
        let (segment_body, segment_corrupt) = read_snapshot_segment(segment)?;
        body.extend_from_slice(&segment_body);
        corrupt = corrupt.saturating_add(segment_corrupt);
    }
    Ok((body, corrupt))
}

impl SpillReadSegment {
    fn open(segment: &SpillSegment) -> io::Result<Self> {
        let data = File::open(&segment.path)?;
        let index = match File::open(&segment.idx_path) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            data,
            index,
            compression: segment.compression,
            data_len: segment.bytes,
            index_len: segment.index_bytes,
        })
    }
}

fn read_snapshot_segment(segment: SpillReadSegment) -> io::Result<(Vec<u8>, u64)> {
    let mut data = Vec::new();
    segment.data.take(segment.data_len).read_to_end(&mut data)?;
    let Some(index) = segment.index else {
        return read_legacy_segment(segment.compression, &data).map(|body| (body, 0));
    };
    let mut index_data = Vec::new();
    index.take(segment.index_len).read_to_end(&mut index_data)?;
    let index = String::from_utf8(index_data)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut out = Vec::new();
    let mut corrupt = 0u64;
    for line in index.lines() {
        let Some(entry) = parse_index_entry(line) else {
            corrupt = corrupt.saturating_add(1);
            continue;
        };
        let end = entry.offset.saturating_add(entry.len);
        let valid_bounds = match segment.compression {
            TraceSpillCompression::None => {
                end <= data.len()
                    && data
                        .get(end)
                        .map(|byte| *byte == b'\n')
                        .unwrap_or(end == data.len())
            }
            TraceSpillCompression::Zstd { .. } => end <= data.len(),
        };
        if !valid_bounds {
            corrupt = corrupt.saturating_add(1);
            continue;
        }
        let record = &data[entry.offset..end];
        let payload = match decode_indexed_record(segment.compression, record) {
            Ok(payload) => payload,
            Err(_) => {
                corrupt = corrupt.saturating_add(1);
                continue;
            }
        };
        if crc32(&payload) != entry.crc {
            corrupt = corrupt.saturating_add(1);
            continue;
        }
        out.extend_from_slice(&payload);
        out.push(b'\n');
    }
    Ok((out, corrupt))
}

fn read_legacy_segment(compression: TraceSpillCompression, data: &[u8]) -> io::Result<Vec<u8>> {
    match compression {
        TraceSpillCompression::None => Ok(data.to_vec()),
        TraceSpillCompression::Zstd { .. } => zstd::stream::decode_all(data),
    }
}
