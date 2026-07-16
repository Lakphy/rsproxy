//! Persistence for the versioned launcher manifest consumed by `rsproxy startup launch`.

use crate::{CliError, CliResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const STARTUP_MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartupManifest {
    pub(super) version: u32,
    pub(super) storage: PathBuf,
    pub(super) config: Option<PathBuf>,
    pub(super) system_proxy: bool,
    pub(super) service: Option<String>,
    pub(super) bypass: Option<Vec<String>>,
    pub(super) proxy_host: String,
    pub(super) proxy_port: u16,
}

pub(super) fn write_manifest(manifest: &StartupManifest) -> CliResult<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| CliError::Json {
        context: "serialize startup manifest",
        source,
    })?;
    Ok(rsproxy_platform::startup::write_startup_manifest(&bytes)?)
}

pub(super) fn remove_manifest() -> CliResult<()> {
    Ok(rsproxy_platform::startup::remove_startup_manifest()?)
}

fn read_manifest_optional(path: &Path) -> CliResult<Option<StartupManifest>> {
    match fs::read(path) {
        Ok(bytes) => parse_manifest(&bytes).map(Some),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::io(
            format!("read startup manifest {}", path.display()),
            source,
        )),
    }
}

/// Reads the manifest without failing on corrupt or version-mismatched contents, so `status`
/// stays informative and `uninstall` keeps a working path to remove the login item.
pub(super) fn read_manifest_lenient(path: &Path) -> (Option<StartupManifest>, Option<String>) {
    match read_manifest_optional(path) {
        Ok(manifest) => (manifest, None),
        Err(error) => (
            None,
            Some(format!(
                "startup manifest {} is unreadable: {error}",
                path.display()
            )),
        ),
    }
}

pub(super) fn read_manifest_required(path: &Path) -> CliResult<StartupManifest> {
    read_manifest_optional(path)?.ok_or_else(|| {
        CliError::Usage(format!(
            "startup manifest {} is missing; run `rsproxy startup install` again",
            path.display()
        ))
    })
}

pub(super) fn parse_manifest(bytes: &[u8]) -> CliResult<StartupManifest> {
    let manifest: StartupManifest =
        serde_json::from_slice(bytes).map_err(|source| CliError::Json {
            context: "parse startup manifest",
            source,
        })?;
    if manifest.version != STARTUP_MANIFEST_VERSION {
        return Err(CliError::Usage(format!(
            "startup manifest version {} is unsupported; expected {}",
            manifest.version, STARTUP_MANIFEST_VERSION
        )));
    }
    Ok(manifest)
}
