use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

use super::workflow_contracts::{CONTRACTS, GLOBAL_REJECTED};
use super::{CheckError, CheckKind, Violation, fail_if_any, io_error};

pub(super) fn check(root: &Path) -> Result<String, CheckError> {
    let violations = violations(root)?;
    fail_if_any(CheckKind::Workflows, violations)?;
    Ok("CI, fuzz, performance, and release workflows satisfy the repository contract.".to_owned())
}

fn violations(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let directory = root.join(".github/workflows");
    let mut violations = inventory_violations(&directory)?;
    for contract in CONTRACTS {
        let relative = PathBuf::from(".github/workflows").join(contract.file);
        let path = root.join(&relative);
        if !path.is_file() {
            violations.push(Violation::new(relative, "required workflow is missing"));
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|source| io_error("read", &path, source))?;
        if source.contains('\t') {
            violations.push(Violation::new(&relative, "workflow contains a tab"));
        }
        for required in contract.required {
            if !source.contains(required) {
                violations.push(Violation::new(
                    &relative,
                    format!("missing required contract text `{required}`"),
                ));
            }
        }
        for rejected in GLOBAL_REJECTED.iter().chain(contract.rejected) {
            if source.contains(rejected) {
                violations.push(Violation::new(
                    &relative,
                    format!("forbidden workflow text `{rejected}`"),
                ));
            }
        }
        if contract.file == "ci.yml" && source.matches("cargo xtask check all").count() < 2 {
            violations.push(Violation::new(
                &relative,
                "CI must run `cargo xtask check all` in both the workspace matrix and repository-contract job",
            ));
        }
        match YamlLoader::load_from_str(&source) {
            Ok(documents) if documents.len() == 1 => {
                stable_action_violations(&documents[0], &relative, &mut violations);
                command_violations(&documents[0], contract.file, &relative, &mut violations);
            }
            Ok(documents) => violations.push(Violation::new(
                &relative,
                format!(
                    "workflow must contain one YAML document, found {}",
                    documents.len()
                ),
            )),
            Err(error) => violations.push(Violation::new(
                &relative,
                format!("invalid YAML syntax: {error}"),
            )),
        }
    }
    Ok(violations)
}

fn inventory_violations(directory: &Path) -> Result<Vec<Violation>, CheckError> {
    let entries = fs::read_dir(directory).map_err(|source| io_error("list", directory, source))?;
    let actual = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| io_error("list", directory, source))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yml" || extension == "yaml")
        })
        .filter_map(|path| path.file_name().map(|name| name.to_owned()))
        .collect::<BTreeSet<_>>();
    let expected = CONTRACTS
        .iter()
        .map(|contract| contract.file.into())
        .collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(Vec::new())
    } else {
        Ok(vec![Violation::new(
            ".github/workflows",
            "workflow inventory must contain exactly ci.yml, fuzz.yml, performance.yml, and release.yml",
        )])
    }
}

fn stable_action_violations(yaml: &Yaml, path: &Path, violations: &mut Vec<Violation>) {
    match yaml {
        Yaml::Array(values) => {
            for value in values {
                stable_action_violations(value, path, violations);
            }
        }
        Yaml::Hash(values) => {
            for (key, value) in values {
                if key.as_str() == Some("uses") {
                    match value.as_str() {
                        Some(action) if stable_action(action) => {}
                        Some(action) => violations.push(Violation::new(
                            path,
                            format!("action reference is not stable: `{action}`"),
                        )),
                        None => violations
                            .push(Violation::new(path, "action `uses` value must be a string")),
                    }
                }
                stable_action_violations(value, path, violations);
            }
        }
        _ => {}
    }
}

fn stable_action(action: &str) -> bool {
    if action.starts_with("./") {
        return true;
    }
    if let Some(image) = action.strip_prefix("docker://") {
        return image.contains("@sha256:");
    }
    let Some((repository, reference)) = action.rsplit_once('@') else {
        return false;
    };
    if !repository.contains('/') {
        return false;
    }
    if repository == "dtolnay/rust-toolchain" {
        return matches!(reference, "stable" | "nightly") || numeric_version(reference);
    }
    version_tag(reference) || commit_hash(reference)
}

fn command_violations(yaml: &Yaml, workflow: &str, path: &Path, violations: &mut Vec<Violation>) {
    let mut commands = Vec::new();
    collect_strings_for_key(yaml, "run", &mut commands);
    let required: &[&str] = match workflow {
        "ci.yml" => &["cargo xtask check all", "cargo xtask check all"],
        "performance.yml" => &[
            "cargo xtask targets criterion target/performance/criterion.json",
            "cargo xtask targets regression \"$RUNNER_TEMP/criterion-base.json\" target/performance/criterion.json 10",
        ],
        "release.yml" => &["cargo xtask release \"$version\" --check"],
        _ => &[],
    };
    let mut available = commands;
    for expected in required {
        if let Some(index) = available
            .iter()
            .position(|command| command.contains(expected))
        {
            available.remove(index);
        } else {
            violations.push(Violation::new(
                path,
                format!("required command is not executed by a `run` step: `{expected}`"),
            ));
        }
    }
    if workflow == "ci.yml" {
        let mut actions = Vec::new();
        collect_strings_for_key(yaml, "uses", &mut actions);
        if !actions.contains(&"EmbarkStudios/cargo-deny-action@v2") {
            violations.push(Violation::new(
                path,
                "CI must execute EmbarkStudios/cargo-deny-action@v2",
            ));
        }
    }
}

fn collect_strings_for_key<'a>(yaml: &'a Yaml, expected: &str, output: &mut Vec<&'a str>) {
    match yaml {
        Yaml::Array(values) => {
            for value in values {
                collect_strings_for_key(value, expected, output);
            }
        }
        Yaml::Hash(values) => {
            for (key, value) in values {
                if key.as_str() == Some(expected)
                    && let Some(value) = value.as_str()
                {
                    output.push(value);
                }
                collect_strings_for_key(value, expected, output);
            }
        }
        _ => {}
    }
}

fn version_tag(reference: &str) -> bool {
    reference.strip_prefix('v').is_some_and(numeric_version)
}

fn numeric_version(reference: &str) -> bool {
    !reference.is_empty()
        && reference
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && reference.split('.').all(|component| !component.is_empty())
}

fn commit_hash(reference: &str) -> bool {
    reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
pub(super) fn violations_for_test(root: &Path) -> Result<Vec<Violation>, CheckError> {
    violations(root)
}

#[cfg(test)]
pub(super) fn stable_action_for_test(action: &str) -> bool {
    stable_action(action)
}
