use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CheckError, CheckKind, Violation, fail_if_any, io_error};

const PUBLIC_API_TOOLCHAIN: &str = "nightly-2026-07-10";
const API_CRATES: &[&str] = &[
    "rsproxy-rules",
    "rsproxy-trace",
    "rsproxy-net",
    "rsproxy-platform",
    "rsproxy-engine",
    "rsproxy-control",
    "rsproxy-cli",
];

pub(super) fn check(root: &Path, bless: bool) -> Result<String, CheckError> {
    let mut violations = Vec::new();
    for package in API_CRATES {
        check_package(root, package, bless, &mut violations)?;
    }
    fail_if_any(CheckKind::Api, violations)?;
    if bless {
        Ok(format!(
            "Updated {} Rust public-API snapshots.",
            API_CRATES.len()
        ))
    } else {
        Ok(format!(
            "{} Rust public-API snapshots match the workspace facade.",
            API_CRATES.len()
        ))
    }
}

fn check_package(
    root: &Path,
    package: &str,
    bless: bool,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    let crate_dir = root.join("crates").join(package);
    let manifest = crate_dir.join("Cargo.toml");
    let relative_snapshot = PathBuf::from("crates").join(package).join("api.txt");
    let snapshot = root.join(&relative_snapshot);
    let output = Command::new("cargo")
        .current_dir(root)
        .arg(format!("+{PUBLIC_API_TOOLCHAIN}"))
        .args(["public-api", "--manifest-path"])
        .arg(&manifest)
        .args(["--color", "never", "--cap-lints", "allow", "-ss"])
        .output()
        .map_err(|source| io_error("run cargo public-api for", &manifest, source))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("cargo public-api failed without an error message");
        violations.push(Violation::new(
            relative_snapshot,
            format!(
                "could not generate `{package}` with cargo-public-api 0.52.0 and {PUBLIC_API_TOOLCHAIN}; ensure both pinned tools are installed: {detail}"
            ),
        ));
        return Ok(());
    }

    let generated = String::from_utf8_lossy(&output.stdout);
    reconcile_snapshot(&snapshot, &relative_snapshot, &generated, bless, violations)
}

fn reconcile_snapshot(
    snapshot: &Path,
    relative_snapshot: &Path,
    generated: &str,
    bless: bool,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    let generated = normalize_snapshot(generated);
    if bless {
        fs::write(snapshot, generated)
            .map_err(|source| io_error("write public API snapshot", snapshot, source))?;
        return Ok(());
    }
    let expected = match fs::read_to_string(snapshot) {
        Ok(expected) => normalize_snapshot(&expected),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            violations.push(Violation::new(
                relative_snapshot,
                "public API snapshot is missing; run `cargo xtask check api --bless`",
            ));
            return Ok(());
        }
        Err(source) => return Err(io_error("read public API snapshot", snapshot, source)),
    };
    if expected != generated {
        violations.push(Violation::new(
            relative_snapshot,
            drift_message(&expected, &generated),
        ));
    }
    Ok(())
}

fn normalize_snapshot(input: &str) -> String {
    let mut normalized = input.replace("\r\n", "\n");
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn drift_message(expected: &str, generated: &str) -> String {
    let expected_lines = expected.lines().collect::<Vec<_>>();
    let generated_lines = generated.lines().collect::<Vec<_>>();
    let line = (0..expected_lines.len().max(generated_lines.len()))
        .find(|&index| expected_lines.get(index) != generated_lines.get(index))
        .unwrap_or(0);
    format!(
        "public API changed at line {} (snapshot: {}, generated: {}); review the API diff, then run `cargo xtask check api --bless`",
        line + 1,
        display_line(expected_lines.get(line).copied()),
        display_line(generated_lines.get(line).copied()),
    )
}

fn display_line(line: Option<&str>) -> String {
    let Some(line) = line else {
        return "<end of file>".to_owned();
    };
    let mut display = line.chars().take(120).collect::<String>();
    if line.chars().count() > 120 {
        display.push('…');
    }
    format!("{display:?}")
}

#[cfg(test)]
pub(super) fn reconcile_snapshot_for_test(
    snapshot: &Path,
    relative_snapshot: &Path,
    generated: &str,
    bless: bool,
    violations: &mut Vec<Violation>,
) -> Result<(), CheckError> {
    reconcile_snapshot(snapshot, relative_snapshot, generated, bless, violations)
}
