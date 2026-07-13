use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::fs_walk;
use super::{CheckError, CheckKind, Violation, fail_if_any, io_error};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    lines: LinesConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinesConfig {
    limit: usize,
    exclude: Vec<PathBuf>,
}

pub(super) fn check(root: &Path) -> Result<String, CheckError> {
    let config = load_config(root)?;
    let violations = find_violations(root, &config.lines)?;
    fail_if_any(CheckKind::Lines, violations)?;
    Ok(format!(
        "All Rust files are at or below {} lines.",
        config.lines.limit
    ))
}

fn load_config(root: &Path) -> Result<Config, CheckError> {
    let path = root.join("xtask.toml");
    let source = std::fs::read_to_string(&path)
        .map_err(|source| io_error("read check configuration", &path, source))?;
    let config: Config = toml::from_str(&source).map_err(|source| CheckError::Config {
        path: path.clone(),
        source,
    })?;
    if config.lines.limit == 0 {
        return Err(super::CheckFailures {
            kind: CheckKind::Lines,
            violations: vec![Violation::new("xtask.toml", "lines.limit must be positive")],
        }
        .into());
    }
    let invalid = config.lines.exclude.iter().find(|path| {
        path.as_os_str().is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
    });
    if let Some(path) = invalid {
        return Err(super::CheckFailures {
            kind: CheckKind::Lines,
            violations: vec![Violation::new(
                "xtask.toml",
                format!("invalid relative exclusion `{}`", path.display()),
            )],
        }
        .into());
    }
    Ok(config)
}

fn find_violations(root: &Path, config: &LinesConfig) -> Result<Vec<Violation>, CheckError> {
    let files = fs_walk::files(root, &["crates", "fuzz"], &config.exclude)?;
    let mut violations = Vec::new();
    for relative in files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
    {
        let contents = fs_walk::read_bytes(root, &relative)?;
        let lines = contents.iter().filter(|&&byte| byte == b'\n').count();
        if lines > config.limit {
            violations.push(Violation::new(
                relative,
                format!("{lines} lines exceeds configured limit {}", config.limit),
            ));
        }
    }
    Ok(violations)
}
