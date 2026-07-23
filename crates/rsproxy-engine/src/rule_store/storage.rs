use super::*;
use rsproxy_rules::{
    MAX_RULE_GROUP_NAME_BYTES, MAX_RULE_GROUPS_PER_SNAPSHOT, MAX_RULE_SNAPSHOT_SOURCE_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MANIFEST_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "groups.toml";
const MAX_RULE_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuleManifest {
    version: u32,
    groups: Vec<ManifestGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestGroup {
    name: String,
    enabled: bool,
}

pub(super) fn load_snapshot(rules_dir: &Path) -> Result<RuleSnapshot, RuleStoreError> {
    let manifest_path = rules_dir.join(MANIFEST_FILE);
    let mut source_bytes = 0usize;
    let mut groups = if manifest_path.is_file() {
        load_manifest_groups(rules_dir, &manifest_path, &mut source_bytes)?
    } else {
        discover_groups(rules_dir, &mut source_bytes)?
    };
    append_unlisted_groups(rules_dir, &mut groups, &mut source_bytes)?;
    if !groups.iter().any(|group| group.name == "default") {
        if groups.len() == MAX_RULE_GROUPS_PER_SNAPSHOT {
            return Err(RuleStoreError::Invalid(format!(
                "stored rules exceed the {MAX_RULE_GROUPS_PER_SNAPSHOT}-group limit"
            )));
        }
        let text = read_group_text(rules_dir, "default", &mut source_bytes)?.unwrap_or_default();
        groups.insert(
            0,
            RuleGroup {
                name: "default".to_string(),
                enabled: true,
                text,
            },
        );
    }
    RuleSnapshot::compile(groups)
}

fn load_manifest_groups(
    rules_dir: &Path,
    manifest_path: &Path,
    source_bytes: &mut usize,
) -> Result<Vec<RuleGroup>, RuleStoreError> {
    let text = read_utf8_file_bounded(manifest_path, MAX_RULE_MANIFEST_BYTES, "groups manifest")?
        .ok_or_else(|| {
        RuleStoreError::Invalid("groups manifest disappeared while reading".to_string())
    })?;
    let manifest: RuleManifest = toml::from_str(&text)
        .map_err(|error| RuleStoreError::Invalid(format!("invalid groups manifest: {error}")))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(RuleStoreError::Invalid(format!(
            "unsupported groups manifest version {}",
            manifest.version
        )));
    }
    if manifest.groups.len() > MAX_RULE_GROUPS_PER_SNAPSHOT {
        return Err(RuleStoreError::Invalid(format!(
            "groups manifest exceeds the {MAX_RULE_GROUPS_PER_SNAPSHOT}-group limit"
        )));
    }
    let mut seen = HashSet::new();
    let mut groups = Vec::with_capacity(manifest.groups.len());
    for entry in manifest.groups {
        validate_group_name(&entry.name)?;
        if !seen.insert(entry.name.clone()) {
            return Err(RuleStoreError::Invalid(format!(
                "duplicate rule group `{}` in manifest",
                entry.name
            )));
        }
        let text = read_group_text(rules_dir, &entry.name, source_bytes)?.ok_or_else(|| {
            RuleStoreError::Invalid(format!(
                "groups manifest references missing rule group `{}`",
                entry.name
            ))
        })?;
        groups.push(RuleGroup {
            name: entry.name,
            enabled: entry.enabled,
            text,
        });
    }
    Ok(groups)
}

fn discover_groups(
    rules_dir: &Path,
    source_bytes: &mut usize,
) -> Result<Vec<RuleGroup>, RuleStoreError> {
    let mut names = discover_group_names(rules_dir)?;
    names.sort_by_key(|name| (name != "default", name.clone()));
    let mut groups = Vec::with_capacity(names.len());
    for name in names {
        groups.push(RuleGroup {
            text: read_group_text(rules_dir, &name, source_bytes)?.unwrap_or_default(),
            name,
            enabled: true,
        });
    }
    Ok(groups)
}

fn append_unlisted_groups(
    rules_dir: &Path,
    groups: &mut Vec<RuleGroup>,
    source_bytes: &mut usize,
) -> Result<(), RuleStoreError> {
    let listed = groups
        .iter()
        .map(|group| group.name.clone())
        .collect::<HashSet<_>>();
    for name in discover_group_names(rules_dir)? {
        if listed.contains(&name) {
            continue;
        }
        if groups.len() == MAX_RULE_GROUPS_PER_SNAPSHOT {
            return Err(RuleStoreError::Invalid(format!(
                "stored rules exceed the {MAX_RULE_GROUPS_PER_SNAPSHOT}-group limit"
            )));
        }
        groups.push(RuleGroup {
            text: read_group_text(rules_dir, &name, source_bytes)?.unwrap_or_default(),
            name,
            enabled: true,
        });
    }
    Ok(())
}

fn discover_group_names(rules_dir: &Path) -> Result<Vec<String>, RuleStoreError> {
    let entries = match fs::read_dir(rules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io_error("read rules directory", source)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| io_error("read rules directory entry", source))?;
        if !entry
            .file_type()
            .map_err(|source| io_error("read rule group file type", source))?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("rules") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        validate_group_name(name)?;
        if names.len() == MAX_RULE_GROUPS_PER_SNAPSHOT {
            return Err(RuleStoreError::Invalid(format!(
                "rules directory exceeds the {MAX_RULE_GROUPS_PER_SNAPSHOT}-group limit"
            )));
        }
        names.push(name.to_string());
    }
    names.sort();
    Ok(names)
}

fn read_group_text(
    rules_dir: &Path,
    name: &str,
    source_bytes: &mut usize,
) -> Result<Option<String>, RuleStoreError> {
    let remaining = MAX_RULE_SNAPSHOT_SOURCE_BYTES.saturating_sub(*source_bytes);
    let Some(text) = read_utf8_file_bounded(
        &group_path(rules_dir, name),
        remaining,
        &format!("rule group `{name}`"),
    )?
    else {
        return Ok(None);
    };
    *source_bytes += text.len();
    Ok(Some(text))
}

fn read_utf8_file_bounded(
    path: &Path,
    limit: usize,
    label: &str,
) -> Result<Option<String>, RuleStoreError> {
    let bytes = match crate::bounded_io::read_file(path, limit, label) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        // The only InvalidData a raw bounded read produces is the limit check.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return Err(RuleStoreError::Invalid(error.to_string()));
        }
        Err(source) => return Err(io_error(&format!("read {label}"), source)),
    };
    let text = String::from_utf8(bytes).map_err(|source| {
        io_error(
            &format!("read {label}"),
            io::Error::new(io::ErrorKind::InvalidData, source),
        )
    })?;
    Ok(Some(text))
}

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;

pub(super) fn persist_manifest(
    rules_dir: &Path,
    groups: &[RuleGroup],
) -> Result<(), RuleStoreError> {
    fs::create_dir_all(rules_dir).map_err(|source| io_error("create rules directory", source))?;
    for group in groups {
        let path = group_path(rules_dir, &group.name);
        if !path.is_file() {
            atomic_write(&path, group.text.as_bytes())?;
        }
    }
    let manifest = RuleManifest {
        version: MANIFEST_VERSION,
        groups: groups
            .iter()
            .map(|group| ManifestGroup {
                name: group.name.clone(),
                enabled: group.enabled,
            })
            .collect(),
    };
    let text = toml::to_string_pretty(&manifest)
        .map_err(|error| RuleStoreError::Invalid(format!("serialize groups manifest: {error}")))?;
    atomic_write(&rules_dir.join(MANIFEST_FILE), text.as_bytes())
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RuleStoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error("create rule path", source))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rules");
    let temp = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&temp, bytes).map_err(|source| io_error("write temporary rule file", source))?;
    if let Err(source) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(io_error("replace rule file", source));
    }
    Ok(())
}

pub(super) fn group_path(rules_dir: &Path, name: &str) -> PathBuf {
    rules_dir.join(format!("{name}.rules"))
}

pub(super) fn validate_group_name(name: &str) -> Result<(), RuleStoreError> {
    if name.is_empty()
        || name.len() > MAX_RULE_GROUP_NAME_BYTES
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(RuleStoreError::Invalid(format!(
            "invalid rule group name `{name}`; use 1-{MAX_RULE_GROUP_NAME_BYTES} ASCII letters, digits, dot, underscore, or hyphen"
        )));
    }
    Ok(())
}

pub(super) fn io_error(context: &str, source: io::Error) -> RuleStoreError {
    RuleStoreError::Io {
        context: context.to_string(),
        source,
    }
}
