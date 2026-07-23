use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) fn read_file(path: &Path, limit: usize, label: &str) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    read_open_file(file, path, limit, label)
}

pub(crate) fn read_open_file<R: Read>(
    file: R,
    display_path: &Path,
    limit: usize,
    label: &str,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{label} {} exceeds the {limit}-byte limit",
                display_path.display()
            ),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
