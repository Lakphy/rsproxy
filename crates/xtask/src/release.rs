mod cargo_files;
mod json_files;
mod transaction;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;
use thiserror::Error;

const ROOT_MANIFEST: &str = "package.json";
const NPM_ROOT: &str = "packages/npm";

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("failed to read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse JSON in {path}")]
    ParseJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to parse TOML in {path}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },

    #[error("invalid release input in {path}: {message}")]
    Invalid { path: PathBuf, message: String },

    #[error("release {version} is not synchronized: {files}")]
    Inconsistent { version: Version, files: String },

    #[error("failed to stage release update for {path}")]
    Stage {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to verify {path} before committing release update")]
    Verify {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} changed while the release update was being prepared")]
    ConcurrentModification { path: PathBuf },

    #[error("failed to commit release update for {path}{rollback}")]
    Commit {
        path: PathBuf,
        rollback: String,
        #[source]
        source: io::Error,
    },

    #[error("release updates succeeded, but backup cleanup failed: {details}")]
    Cleanup { details: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseOutcome {
    pub changed_files: usize,
}

#[derive(Debug)]
struct FileChange {
    path: PathBuf,
    original: Vec<u8>,
    updated: Vec<u8>,
}

impl FileChange {
    fn text(path: PathBuf, original: String, updated: String) -> Option<Self> {
        (original != updated).then(|| Self {
            path,
            original: original.into_bytes(),
            updated: updated.into_bytes(),
        })
    }
}

pub fn release(
    workspace_root: &Path,
    version: &Version,
    check: bool,
) -> Result<ReleaseOutcome, ReleaseError> {
    let inventory =
        json_files::read_target_inventory(&workspace_root.join(NPM_ROOT).join("targets.json"))?;
    let (cargo_change, workspace_packages) =
        cargo_files::plan_workspace_manifest(workspace_root, version)?;

    let mut changes = Vec::new();
    push_change(&mut changes, cargo_change);
    push_change(
        &mut changes,
        cargo_files::plan_lockfile(
            &workspace_root.join("Cargo.lock"),
            &workspace_packages,
            version,
            true,
        )?,
    );

    let fuzz_lock = workspace_root.join("fuzz/Cargo.lock");
    if fuzz_lock.exists() {
        push_change(
            &mut changes,
            cargo_files::plan_lockfile(&fuzz_lock, &workspace_packages, version, false)?,
        );
    }

    push_change(
        &mut changes,
        json_files::plan_root_manifest(&workspace_root.join(ROOT_MANIFEST), version)?,
    );
    push_change(
        &mut changes,
        json_files::plan_runtime_manifest(
            &workspace_root.join(NPM_ROOT).join("runtime/package.json"),
            version,
            &inventory,
        )?,
    );
    for launcher in ["cli", "bun"] {
        push_change(
            &mut changes,
            json_files::plan_launcher_manifest(
                &workspace_root
                    .join(NPM_ROOT)
                    .join(launcher)
                    .join("package.json"),
                launcher,
                version,
            )?,
        );
    }

    if check && !changes.is_empty() {
        let files = changes
            .iter()
            .map(|change| display_relative(workspace_root, &change.path))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ReleaseError::Inconsistent {
            version: version.clone(),
            files,
        });
    }

    let changed_files = changes.len();
    if !check {
        transaction::apply(&changes)?;
    }
    Ok(ReleaseOutcome { changed_files })
}

fn push_change(changes: &mut Vec<FileChange>, change: Option<FileChange>) {
    if let Some(change) = change {
        changes.push(change);
    }
}

fn read_text(path: &Path) -> Result<String, ReleaseError> {
    fs::read_to_string(path).map_err(|source| ReleaseError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn invalid(path: &Path, message: impl Into<String>) -> ReleaseError {
    ReleaseError::Invalid {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests;
