use super::TraceSpillCompression;

pub(super) fn encode_spill_record(
    line: &str,
    compression: TraceSpillCompression,
) -> std::io::Result<Vec<u8>> {
    match compression {
        TraceSpillCompression::None => {
            let mut record = Vec::with_capacity(line.len() + 1);
            record.extend_from_slice(line.as_bytes());
            record.push(b'\n');
            Ok(record)
        }
        TraceSpillCompression::Zstd { level } => {
            let mut record = Vec::with_capacity(line.len() + 1);
            record.extend_from_slice(line.as_bytes());
            record.push(b'\n');
            zstd::stream::encode_all(record.as_slice(), level)
        }
    }
}

pub(super) fn decode_indexed_record(
    compression: TraceSpillCompression,
    record: &[u8],
) -> std::io::Result<Vec<u8>> {
    match compression {
        TraceSpillCompression::None => Ok(record.to_vec()),
        TraceSpillCompression::Zstd { .. } => {
            let decoded = zstd::stream::decode_all(record)?;
            decoded
                .strip_suffix(b"\n")
                .map(|payload| payload.to_vec())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "compressed spill record is missing trailing newline",
                    )
                })
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpillIndexEntry {
    pub(crate) offset: usize,
    pub(crate) len: usize,
    pub(crate) crc: u32,
}

pub(crate) fn parse_index_entry(line: &str) -> Option<SpillIndexEntry> {
    let mut parts = line.split_whitespace();
    let offset = parts.next()?.parse::<usize>().ok()?;
    let len = parts.next()?.parse::<usize>().ok()?;
    let _id = parts.next()?.parse::<u64>().ok()?;
    let crc = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some(SpillIndexEntry { offset, len, crc })
}

pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
