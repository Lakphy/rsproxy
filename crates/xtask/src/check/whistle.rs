use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::fs_walk;
use super::{CheckError, Violation};

const VERSION: &str = "2.10.5";
const EVIDENCE_FILES: usize = 75;
const FIXTURE: &str = "crates/rsproxy-rules/tests/fixtures/whistle-2.10.5";
const DRIVER: &str = "benches/e2e/whistle-driver";

#[derive(Debug, Deserialize)]
struct Snapshot {
    schema: String,
    upstream: String,
    version: String,
    commit: String,
    license: String,
    evidence_files: usize,
}

pub(super) fn violations(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let mut violations = Vec::new();
    match std::fs::symlink_metadata(root.join("whistle")) {
        Ok(_) => violations.push(Violation::new(
            "whistle",
            "root Whistle checkout must not exist",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(super::io_error("inspect", &root.join("whistle"), source)),
    }
    let fixture = Path::new(FIXTURE);
    check_snapshot(root, fixture, &mut violations)?;
    check_evidence_count(root, fixture, &mut violations)?;
    check_hashes(root, fixture, &mut violations)?;
    check_driver(root, &mut violations)?;
    check_active_references(root, &mut violations)?;
    Ok(violations)
}

fn check_snapshot(
    root: &Path,
    fixture: &Path,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    for name in ["SNAPSHOT.toml", "SHA256SUMS", "LICENSE"] {
        let relative = fixture.join(name);
        if !root.join(&relative).is_file() {
            violations.push(Violation::new(relative, "required fixture file is missing"));
        }
    }
    let relative = fixture.join("SNAPSHOT.toml");
    if !root.join(&relative).is_file() {
        return Ok(());
    }
    let source = fs_walk::read_text(root, &relative)?;
    let snapshot: Snapshot = toml::from_str(&source).map_err(|source| CheckError::Config {
        path: relative.clone(),
        source,
    })?;
    let valid_commit =
        snapshot.commit.len() == 40 && snapshot.commit.bytes().all(|byte| byte.is_ascii_hexdigit());
    if snapshot.schema != "rsproxy.whistle-fixture/v1"
        || snapshot.upstream != "https://github.com/avwo/whistle"
        || snapshot.version != VERSION
        || snapshot.license != "MIT"
        || snapshot.evidence_files != EVIDENCE_FILES
        || !valid_commit
    {
        violations.push(Violation::new(
            relative,
            "snapshot metadata does not match the pinned Whistle 2.10.5 contract",
        ));
    }
    Ok(())
}

fn check_evidence_count(
    root: &Path,
    fixture: &Path,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    let mut count = 0;
    for directory in ["docs", "lib", "test"] {
        let relative = fixture.join(directory);
        if !root.join(&relative).is_dir() {
            violations.push(Violation::new(relative, "evidence directory is missing"));
            continue;
        }
        count += fs_walk::files_in(root, &relative)?.len();
    }
    if count != EVIDENCE_FILES {
        violations.push(Violation::new(
            fixture,
            format!("expected {EVIDENCE_FILES} evidence files, found {count}"),
        ));
    }
    Ok(())
}

fn check_hashes(
    root: &Path,
    fixture: &Path,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    let sums = fixture.join("SHA256SUMS");
    if !root.join(&sums).is_file() {
        return Ok(());
    }
    let source = fs_walk::read_text(root, &sums)?;
    let mut seen = HashSet::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        let Some((expected, name)) = line.split_once("  ") else {
            violations.push(Violation::new(
                &sums,
                format!("line {} is not a SHA256SUMS entry", index + 1),
            ));
            continue;
        };
        let name = PathBuf::from(name);
        if expected.len() != 64
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !safe_relative(&name)
            || !seen.insert(name.clone())
        {
            violations.push(Violation::new(
                &sums,
                format!("line {} has an invalid hash or path", index + 1),
            ));
            continue;
        }
        let relative = fixture.join(&name);
        if !root.join(&relative).is_file() {
            violations.push(Violation::new(relative, "hashed fixture file is missing"));
            continue;
        }
        let actual = format!(
            "{:x}",
            Sha256::digest(fs_walk::read_bytes(root, &relative)?)
        );
        if !actual.eq_ignore_ascii_case(expected) {
            violations.push(Violation::new(relative, "SHA-256 digest does not match"));
        }
    }
    let mut expected_paths = HashSet::from([PathBuf::from("LICENSE")]);
    for directory in ["docs", "lib", "test"] {
        let relative = fixture.join(directory);
        if root.join(&relative).is_dir() {
            expected_paths.extend(
                fs_walk::files_in(root, &relative)?
                    .into_iter()
                    .filter_map(|path| path.strip_prefix(fixture).ok().map(Path::to_path_buf)),
            );
        }
    }
    if seen != expected_paths {
        violations.push(Violation::new(
            sums,
            "SHA256SUMS must cover LICENSE and every evidence file exactly once",
        ));
    }
    Ok(())
}

fn check_driver(root: &Path, violations: &mut Vec<Violation>) -> Result<(), CheckError> {
    let package_path = Path::new(DRIVER).join("package.json");
    let lock_path = Path::new(DRIVER).join("package-lock.json");
    let package = read_json(root, &package_path)?;
    let lock = read_json(root, &lock_path)?;
    if package["dependencies"]["whistle"].as_str() != Some(VERSION) {
        violations.push(Violation::new(
            package_path,
            "benchmark driver must pin whistle 2.10.5",
        ));
    }
    if lock["packages"]["node_modules/whistle"]["version"].as_str() != Some(VERSION) {
        violations.push(Violation::new(
            lock_path,
            "benchmark lockfile must resolve whistle 2.10.5",
        ));
    }
    Ok(())
}

fn check_active_references(root: &Path, violations: &mut Vec<Violation>) -> Result<(), CheckError> {
    let first = ["$ROOT", "/whistle"].concat();
    let second = ["root.join(\"", "whistle/"].concat();
    for relative in fs_walk::files(root, &["crates", "benches", "scripts"], &[])? {
        if relative.ends_with("check-whistle-isolation.sh") {
            continue;
        }
        let bytes = fs_walk::read_bytes(root, &relative)?;
        let source = String::from_utf8_lossy(&bytes);
        if source.contains(&first) || source.contains(&second) {
            violations.push(Violation::new(
                relative,
                "active code references a root Whistle checkout",
            ));
        }
    }
    Ok(())
}

fn read_json(root: &Path, relative: &Path) -> Result<Value, CheckError> {
    let source = fs_walk::read_text(root, relative)?;
    serde_json::from_str(&source).map_err(|source| CheckError::Json {
        path: relative.to_path_buf(),
        source,
    })
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

#[cfg(test)]
pub(super) fn violations_for_test(root: &Path) -> Result<Vec<Violation>, CheckError> {
    violations(root)
}
