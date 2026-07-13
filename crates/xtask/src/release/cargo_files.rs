use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use semver::Version;
use toml_edit::{DocumentMut, Item, value};

use super::{FileChange, ReleaseError, invalid, read_text};

pub(super) fn plan_workspace_manifest(
    workspace_root: &Path,
    version: &Version,
) -> Result<(Option<FileChange>, BTreeSet<String>), ReleaseError> {
    let path = workspace_root.join("Cargo.toml");
    let original = read_text(&path)?;
    let mut document = parse_toml(&path, &original)?;
    let workspace = document
        .get("workspace")
        .and_then(Item::as_table)
        .ok_or_else(|| invalid(&path, "missing `[workspace]` table"))?;
    let package = workspace
        .get("package")
        .and_then(Item::as_table)
        .ok_or_else(|| invalid(&path, "missing `[workspace.package]` table"))?;
    let current = package
        .get("version")
        .and_then(Item::as_str)
        .ok_or_else(|| invalid(&path, "workspace package version must be a string"))?;
    Version::parse(current)
        .map_err(|source| invalid(&path, format!("workspace version `{current}`: {source}")))?;

    let member_paths = workspace
        .get("members")
        .and_then(Item::as_array)
        .ok_or_else(|| invalid(&path, "workspace members must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| invalid(&path, "workspace member paths must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let package_names = read_workspace_package_names(workspace_root, &member_paths)?;
    let version_string = version.to_string();

    if current == version_string {
        return Ok((None, package_names));
    }
    let workspace = document
        .get_mut("workspace")
        .and_then(Item::as_table_mut)
        .expect("workspace table was validated");
    let package = workspace
        .get_mut("package")
        .and_then(Item::as_table_mut)
        .expect("workspace package table was validated");
    package["version"] = value(version_string);
    let updated = document.to_string();
    Ok((FileChange::text(path, original, updated), package_names))
}

pub(super) fn plan_lockfile(
    path: &Path,
    workspace_packages: &BTreeSet<String>,
    version: &Version,
    require_all: bool,
) -> Result<Option<FileChange>, ReleaseError> {
    let original = read_text(path)?;
    let mut document = parse_toml(path, &original)?;
    let packages = document
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| invalid(path, "Cargo lockfile has no package inventory"))?;
    let mut found = BTreeSet::new();
    let mut changed = false;
    let version_string = version.to_string();
    for package in packages.iter_mut() {
        if package.get("source").is_some() {
            continue;
        }
        let Some(name) = package.get("name").and_then(Item::as_str) else {
            return Err(invalid(path, "lockfile package is missing a string name"));
        };
        if !workspace_packages.contains(name) {
            continue;
        }
        found.insert(name.to_owned());
        let current = package
            .get("version")
            .and_then(Item::as_str)
            .ok_or_else(|| invalid(path, format!("lockfile package `{name}` has no version")))?;
        if current != version_string {
            package["version"] = value(version_string.clone());
            changed = true;
        }
    }

    if require_all && found != *workspace_packages {
        let missing = workspace_packages
            .difference(&found)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(
            path,
            format!("workspace packages missing from lockfile: {missing}"),
        ));
    }
    if !changed {
        return Ok(None);
    }
    Ok(FileChange::text(
        path.to_path_buf(),
        original,
        document.to_string(),
    ))
}

fn read_workspace_package_names(
    root: &Path,
    member_paths: &[String],
) -> Result<BTreeSet<String>, ReleaseError> {
    let mut names = BTreeSet::new();
    for member in member_paths {
        if member.contains(['*', '?', '[', ']']) {
            return Err(invalid(
                &root.join("Cargo.toml"),
                format!("workspace member globs are not supported: `{member}`"),
            ));
        }
        let path = root.join(member).join("Cargo.toml");
        let source = read_text(&path)?;
        let document = parse_toml(&path, &source)?;
        let package = document
            .get("package")
            .and_then(Item::as_table)
            .ok_or_else(|| invalid(&path, "missing `[package]` table"))?;
        let name = package
            .get("name")
            .and_then(Item::as_str)
            .ok_or_else(|| invalid(&path, "package name must be a string"))?;
        let inherits_version = package
            .get("version")
            .and_then(Item::as_table_like)
            .and_then(|version| version.get("workspace"))
            .and_then(Item::as_bool)
            == Some(true);
        if !inherits_version {
            return Err(invalid(
                &path,
                "workspace member must set `version.workspace = true`",
            ));
        }
        if !names.insert(name.to_owned()) {
            return Err(invalid(
                &path,
                format!("duplicate workspace package name `{name}`"),
            ));
        }
    }
    Ok(names)
}

fn parse_toml(path: &Path, source: &str) -> Result<DocumentMut, ReleaseError> {
    source.parse().map_err(|source| ReleaseError::ParseToml {
        path: PathBuf::from(path),
        source,
    })
}
