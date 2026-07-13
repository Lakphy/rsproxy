use std::collections::HashSet;
use std::path::Path;

use indexmap::IndexMap;
use semver::Version;
use serde::{Deserialize, Serialize};

use super::{FileChange, ReleaseError, invalid, read_text};

const EXPECTED_TARGETS: usize = 8;
const RUNTIME_PACKAGE: &str = "@rsproxy/runtime";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<JsonValue>),
    Object(IndexMap<String, JsonValue>),
}

#[derive(Debug, Deserialize)]
struct TargetsDocument {
    #[serde(rename = "schemaVersion")]
    schema_version: u64,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    #[serde(rename = "rustTarget")]
    rust_target: String,
    package: String,
}

#[derive(Debug)]
pub(super) struct TargetInventory {
    packages: Vec<String>,
}

pub(super) fn read_target_inventory(path: &Path) -> Result<TargetInventory, ReleaseError> {
    let source = read_text(path)?;
    let document: TargetsDocument =
        serde_json::from_str(&source).map_err(|source| ReleaseError::ParseJson {
            path: path.to_path_buf(),
            source,
        })?;
    if document.schema_version != 1 {
        return Err(invalid(
            path,
            format!(
                "targets schemaVersion must be 1, found {}",
                document.schema_version
            ),
        ));
    }
    if document.targets.len() != EXPECTED_TARGETS {
        return Err(invalid(
            path,
            format!(
                "targets must contain exactly {EXPECTED_TARGETS} entries, found {}",
                document.targets.len()
            ),
        ));
    }

    let mut rust_targets = HashSet::new();
    let mut packages = HashSet::new();
    for target in &document.targets {
        if target.rust_target.is_empty() || !rust_targets.insert(target.rust_target.as_str()) {
            return Err(invalid(
                path,
                format!("duplicate or empty Rust target `{}`", target.rust_target),
            ));
        }
        if !target.package.starts_with("@rsproxy/") || !packages.insert(target.package.as_str()) {
            return Err(invalid(
                path,
                format!("duplicate or invalid npm package `{}`", target.package),
            ));
        }
        if matches!(
            target.package.as_str(),
            "@rsproxy/cli" | "@rsproxy/bun" | RUNTIME_PACKAGE
        ) {
            return Err(invalid(
                path,
                format!(
                    "launcher package cannot be a native target: `{}`",
                    target.package
                ),
            ));
        }
    }

    Ok(TargetInventory {
        packages: document
            .targets
            .into_iter()
            .map(|target| target.package)
            .collect(),
    })
}

pub(super) fn plan_root_manifest(
    path: &Path,
    version: &Version,
) -> Result<Option<FileChange>, ReleaseError> {
    let version = version.to_string();
    mutate_manifest(path, |manifest| {
        set_string(manifest, "version", &version, path)?;
        Ok(())
    })
}

pub(super) fn plan_runtime_manifest(
    path: &Path,
    version: &Version,
    inventory: &TargetInventory,
) -> Result<Option<FileChange>, ReleaseError> {
    let version = version.to_string();
    mutate_manifest(path, |manifest| {
        require_name(manifest, RUNTIME_PACKAGE, path)?;
        set_string(manifest, "version", &version, path)?;
        let dependencies = inventory
            .packages
            .iter()
            .map(|name| (name.clone(), JsonValue::String(version.clone())))
            .collect();
        manifest.insert(
            "optionalDependencies".to_owned(),
            JsonValue::Object(dependencies),
        );
        Ok(())
    })
}

pub(super) fn plan_launcher_manifest(
    path: &Path,
    launcher: &str,
    version: &Version,
) -> Result<Option<FileChange>, ReleaseError> {
    let version = version.to_string();
    mutate_manifest(path, |manifest| {
        require_name(manifest, &format!("@rsproxy/{launcher}"), path)?;
        set_string(manifest, "version", &version, path)?;
        let dependencies = object_field_mut(manifest, "dependencies", path)?;
        if let Some(unexpected) = dependencies
            .keys()
            .find(|name| name.starts_with("@rsproxy/") && name.as_str() != RUNTIME_PACKAGE)
        {
            return Err(invalid(
                path,
                format!("unexpected internal dependency `{unexpected}`"),
            ));
        }
        dependencies.insert(
            RUNTIME_PACKAGE.to_owned(),
            JsonValue::String(version.clone()),
        );
        Ok(())
    })
}

fn mutate_manifest(
    path: &Path,
    mutate: impl FnOnce(&mut IndexMap<String, JsonValue>) -> Result<(), ReleaseError>,
) -> Result<Option<FileChange>, ReleaseError> {
    let original = read_text(path)?;
    let mut document: JsonValue =
        serde_json::from_str(&original).map_err(|source| ReleaseError::ParseJson {
            path: path.to_path_buf(),
            source,
        })?;
    let manifest = match &mut document {
        JsonValue::Object(manifest) => manifest,
        _ => return Err(invalid(path, "top-level JSON value must be an object")),
    };
    let before = manifest.clone();
    mutate(manifest)?;
    if *manifest == before {
        return Ok(None);
    }
    let updated = serde_json::to_string_pretty(&document)
        .expect("serializing a parsed JSON document cannot fail")
        + "\n";
    Ok(FileChange::text(path.to_path_buf(), original, updated))
}

fn require_name(
    manifest: &IndexMap<String, JsonValue>,
    expected: &str,
    path: &Path,
) -> Result<(), ReleaseError> {
    match manifest.get("name") {
        Some(JsonValue::String(name)) if name == expected => Ok(()),
        Some(JsonValue::String(name)) => Err(invalid(
            path,
            format!("expected package name `{expected}`, found `{name}`"),
        )),
        _ => Err(invalid(path, "package name must be a string")),
    }
}

fn set_string(
    manifest: &mut IndexMap<String, JsonValue>,
    field: &str,
    value: &str,
    path: &Path,
) -> Result<(), ReleaseError> {
    match manifest.get_mut(field) {
        Some(JsonValue::String(current)) => {
            value.clone_into(current);
            Ok(())
        }
        Some(_) => Err(invalid(path, format!("`{field}` must be a string"))),
        None => Err(invalid(path, format!("missing required `{field}` field"))),
    }
}

fn object_field_mut<'a>(
    manifest: &'a mut IndexMap<String, JsonValue>,
    field: &str,
    path: &Path,
) -> Result<&'a mut IndexMap<String, JsonValue>, ReleaseError> {
    match manifest.get_mut(field) {
        Some(JsonValue::Object(object)) => Ok(object),
        Some(_) => Err(invalid(path, format!("`{field}` must be an object"))),
        None => Err(invalid(path, format!("missing required `{field}` object"))),
    }
}
