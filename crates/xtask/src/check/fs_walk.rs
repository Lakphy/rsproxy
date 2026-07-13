use std::fs;
use std::path::{Path, PathBuf};

use super::{CheckError, io_error};

pub(super) fn files(
    workspace_root: &Path,
    roots: &[&str],
    excluded: &[PathBuf],
) -> Result<Vec<PathBuf>, CheckError> {
    let mut output = Vec::new();
    for relative in roots {
        let path = workspace_root.join(relative);
        walk(workspace_root, &path, excluded, &mut output)?;
    }
    output.sort();
    Ok(output)
}

pub(super) fn files_in(workspace_root: &Path, relative: &Path) -> Result<Vec<PathBuf>, CheckError> {
    let mut output = Vec::new();
    walk(
        workspace_root,
        &workspace_root.join(relative),
        &[],
        &mut output,
    )?;
    output.sort();
    Ok(output)
}

fn walk(
    workspace_root: &Path,
    path: &Path,
    excluded: &[PathBuf],
    output: &mut Vec<PathBuf>,
) -> Result<(), CheckError> {
    let relative = path.strip_prefix(workspace_root).unwrap_or(path);
    if excluded
        .iter()
        .any(|excluded| relative.starts_with(excluded))
    {
        return Ok(());
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspect", path, source))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        output.push(relative.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(path).map_err(|source| io_error("list", path, source))?;
    let mut children = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| io_error("list", path, source))
        })
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for child in children {
        walk(workspace_root, &child, excluded, output)?;
    }
    Ok(())
}

pub(super) fn read_text(root: &Path, relative: &Path) -> Result<String, CheckError> {
    let path = root.join(relative);
    fs::read_to_string(&path).map_err(|source| io_error("read", &path, source))
}

pub(super) fn read_bytes(root: &Path, relative: &Path) -> Result<Vec<u8>, CheckError> {
    let path = root.join(relative);
    fs::read(&path).map_err(|source| io_error("read", &path, source))
}
