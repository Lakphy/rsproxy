use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{FileChange, ReleaseError};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct StagedFile {
    change_index: usize,
    temporary: PathBuf,
    backup: PathBuf,
}

pub(super) fn apply(changes: &[FileChange]) -> Result<(), ReleaseError> {
    let staged = stage_all(changes)?;
    verify_unchanged(changes, &staged)?;

    let mut committed = Vec::new();
    for (position, staged_file) in staged.iter().enumerate() {
        let change = &changes[staged_file.change_index];
        if let Err(source) = fs::rename(&change.path, &staged_file.backup) {
            remove_staged(&staged[position..]);
            let rollback = rollback(changes, &committed);
            return Err(ReleaseError::Commit {
                path: change.path.clone(),
                rollback,
                source,
            });
        }
        if let Err(source) = fs::rename(&staged_file.temporary, &change.path) {
            remove_staged(&staged[position..]);
            let mut rollback_files = committed;
            rollback_files.push(staged_file.clone());
            let rollback = rollback(changes, &rollback_files);
            return Err(ReleaseError::Commit {
                path: change.path.clone(),
                rollback,
                source,
            });
        }
        committed.push(staged_file.clone());
    }

    remove_backups(&committed)
}

fn stage_all(changes: &[FileChange]) -> Result<Vec<StagedFile>, ReleaseError> {
    let mut staged = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        let staged_file = stage(&change.path, &change.updated).and_then(|temporary| {
            backup_path(&change.path).map(|backup| StagedFile {
                change_index: index,
                temporary,
                backup,
            })
        });
        match staged_file {
            Ok(staged_file) => staged.push(staged_file),
            Err(source) => {
                remove_staged(&staged);
                return Err(ReleaseError::Stage {
                    path: change.path.clone(),
                    source,
                });
            }
        }
    }
    Ok(staged)
}

fn verify_unchanged(changes: &[FileChange], staged: &[StagedFile]) -> Result<(), ReleaseError> {
    for staged_file in staged {
        let change = &changes[staged_file.change_index];
        let current = fs::read(&change.path).map_err(|source| {
            remove_staged(staged);
            ReleaseError::Verify {
                path: change.path.clone(),
                source,
            }
        })?;
        if current != change.original {
            remove_staged(staged);
            return Err(ReleaseError::ConcurrentModification {
                path: change.path.clone(),
            });
        }
    }
    Ok(())
}

fn rollback(changes: &[FileChange], committed: &[StagedFile]) -> String {
    let mut failures = Vec::new();
    for staged_file in committed.iter().rev() {
        let destination = &changes[staged_file.change_index].path;
        match remove_if_present(destination)
            .and_then(|()| fs::rename(&staged_file.backup, destination))
        {
            Ok(()) => {}
            Err(error) => failures.push(format!("{}: {error}", destination.display())),
        }
    }
    if failures.is_empty() {
        "; prior updates were rolled back".to_owned()
    } else {
        format!("; rollback also failed for {}", failures.join(", "))
    }
}

fn stage(destination: &Path, contents: &[u8]) -> io::Result<PathBuf> {
    loop {
        let temporary = sibling_path(destination, "tmp")?;
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            if let Ok(metadata) = fs::metadata(destination) {
                fs::set_permissions(&temporary, metadata.permissions())?;
            }
            Ok::<_, io::Error>(())
        })() {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        return Ok(temporary);
    }
}

fn backup_path(destination: &Path) -> io::Result<PathBuf> {
    loop {
        let backup = sibling_path(destination, "backup")?;
        if !backup.exists() {
            return Ok(backup);
        }
    }
}

fn sibling_path(destination: &Path, suffix: &str) -> io::Result<PathBuf> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{file_name}.rsproxy-release-{}-{sequence}.{suffix}",
        std::process::id()
    )))
}

fn remove_backups(committed: &[StagedFile]) -> Result<(), ReleaseError> {
    let failures = committed
        .iter()
        .filter_map(|file| {
            fs::remove_file(&file.backup)
                .err()
                .map(|error| format!("{}: {error}", file.backup.display()))
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ReleaseError::Cleanup {
            details: failures.join(", "),
        })
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_staged(staged: &[StagedFile]) {
    for staged_file in staged {
        let _ = fs::remove_file(&staged_file.temporary);
    }
}
